// A blocking newline-framed unix-socket client over raw POSIX calls. Deliberately primitive:
// the probe measures the floor (row D) and reports errnos verbatim (row C), so no framework may
// sit between it and the syscalls.

import Foundation

enum ConnectOutcome {
    case connected(LineClient)
    /// connect(2) failed; the errno is the evidence (row C2 expects EPERM under the sandbox).
    case failed(errno: Int32, message: String)
}

final class LineClient {
    private let fd: Int32
    private var buffer = Data()
    private var seq: UInt64 = 0
    private var queuedPushes: [PushW] = []
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    private init(fd: Int32) {
        self.fd = fd
    }

    deinit {
        close(fd)
    }

    static func connect(path: String, timeoutSeconds: Int = 10) -> ConnectOutcome {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            return .failed(errno: errno, message: String(cString: strerror(errno)))
        }
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let ok = withUnsafeMutableBytes(of: &addr.sun_path) { raw -> Bool in
            let bytes = Array(path.utf8)
            guard bytes.count < raw.count else { return false }
            raw.copyBytes(from: bytes)
            raw[bytes.count] = 0
            return true
        }
        guard ok else {
            close(fd)
            return .failed(errno: ENAMETOOLONG, message: "socket path too long")
        }
        let result = withUnsafePointer(to: &addr) { p in
            p.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.connect(fd, sa, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        if result != 0 {
            let e = errno
            close(fd)
            return .failed(errno: e, message: String(cString: strerror(e)))
        }
        var tv = timeval(tv_sec: timeoutSeconds, tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))
        return .connected(LineClient(fd: fd))
    }

    private func sendLine(_ data: Data) -> Bool {
        var out = data
        out.append(0x0A)
        return out.withUnsafeBytes { raw in
            var sent = 0
            while sent < raw.count {
                let n = write(fd, raw.baseAddress!.advanced(by: sent), raw.count - sent)
                if n <= 0 { return false }
                sent += n
            }
            return true
        }
    }

    private func readLine() -> Data? {
        while true {
            if let idx = buffer.firstIndex(of: 0x0A) {
                let line = buffer.prefix(upTo: idx)
                buffer.removeSubrange(...idx)
                return Data(line)
            }
            var chunk = [UInt8](repeating: 0, count: 4096)
            let n = read(fd, &chunk, chunk.count)
            if n <= 0 { return nil }  // EOF, error, or SO_RCVTIMEO expiry
            buffer.append(contentsOf: chunk[0..<n])
        }
    }

    private func readEnvelope() -> ServerEnv? {
        guard let line = readLine() else { return nil }
        return try? decoder.decode(ServerEnv.self, from: line)
    }

    /// Send one request; return its correlated response (queueing pushes that arrive first).
    func request(_ req: Request) -> Resp? {
        seq += 1
        let frame = ClientFrame(v: schemaVersion, seq: seq, req: req)
        guard let data = try? encoder.encode(frame), sendLine(data) else { return nil }
        while true {
            guard let env = readEnvelope() else { return nil }
            switch env.kind {
            case "response" where env.re == seq:
                return env.resp
            case "push":
                if let p = env.push { queuedPushes.append(p) }
            case "refused":
                FileHandle.standardError.write(
                    "refused: \(env.reason ?? "?")\n".data(using: .utf8)!)
                return nil
            default:
                continue
            }
        }
    }

    /// Block (bounded by the read timeout) for the next push.
    func waitPush() -> PushW? {
        if !queuedPushes.isEmpty { return queuedPushes.removeFirst() }
        while true {
            guard let env = readEnvelope() else { return nil }
            if env.kind == "push", let p = env.push { return p }
        }
    }
}
