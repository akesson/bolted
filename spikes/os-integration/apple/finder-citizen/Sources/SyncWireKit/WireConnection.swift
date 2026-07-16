// The UI-grade client the blocking probe LineClient cannot be: a dedicated reader thread owns
// every read and demultiplexes correlated responses (handed back to blocked requesters) from
// unsolicited pushes (delivered on a callback queue). Still no async runtime — std threads and
// semaphores, mirroring the daemon's own concurrency choice.
//
// Friction finding (report this): step 18's wire needs TWO client shapes — a blocking one for
// probes/CLIs and a demultiplexing one for push-driven UIs. A generated client library must
// ship the latter; the former falls out of it.

import Foundation

public final class WireConnection {
    /// Unsolicited daemon ticks, delivered on `callbackQueue` — never the reader thread, so a
    /// handler may issue requests without deadlocking the demultiplexer.
    public var onPush: ((PushW) -> Void)?
    /// EOF/error on the socket, after all in-flight requests have been failed with nil.
    public var onDisconnect: (() -> Void)?

    private let fd: Int32
    private let callbackQueue: DispatchQueue
    private let writeLock = NSLock()
    private let stateLock = NSLock()
    private var pending: [UInt64: (DispatchSemaphore, UnsafeMutablePointer<Resp?>)] = [:]
    private var nextSeq: UInt64 = 0
    private var closed = false
    private let encoder = JSONEncoder()

    private init(fd: Int32, callbackQueue: DispatchQueue) {
        self.fd = fd
        self.callbackQueue = callbackQueue
    }

    deinit {
        close()
    }

    public static func connect(
        path: String,
        callbackQueue: DispatchQueue = DispatchQueue(label: "dev.bolted.sync.wire-pushes")
    ) -> WireConnection? {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { return nil }
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
            Darwin.close(fd)
            return nil
        }
        let result = withUnsafePointer(to: &addr) { p in
            p.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.connect(fd, sa, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard result == 0 else {
            Darwin.close(fd)
            return nil
        }
        let conn = WireConnection(fd: fd, callbackQueue: callbackQueue)
        conn.startReader()
        return conn
    }

    /// Blocking request/response, callable from any thread except the callback queue's push
    /// handler is fine too — the reader thread is the only place this must never run.
    public func request(_ req: Request, timeoutSeconds: Int = 10) -> Resp? {
        stateLock.lock()
        if closed {
            stateLock.unlock()
            return nil
        }
        nextSeq += 1
        let seq = nextSeq
        let sem = DispatchSemaphore(value: 0)
        let slot = UnsafeMutablePointer<Resp?>.allocate(capacity: 1)
        slot.initialize(to: nil)
        pending[seq] = (sem, slot)
        stateLock.unlock()

        defer {
            stateLock.lock()
            pending[seq] = nil
            stateLock.unlock()
            slot.deinitialize(count: 1)
            slot.deallocate()
        }

        let frame = ClientFrame(v: schemaVersion, seq: seq, req: req)
        guard let data = try? encoder.encode(frame), sendLine(data) else { return nil }
        guard sem.wait(timeout: .now() + .seconds(timeoutSeconds)) == .success else { return nil }
        return slot.pointee
    }

    public func close() {
        stateLock.lock()
        let wasClosed = closed
        closed = true
        stateLock.unlock()
        if !wasClosed {
            shutdown(fd, SHUT_RDWR)
        }
    }

    private func sendLine(_ data: Data) -> Bool {
        writeLock.lock()
        defer { writeLock.unlock() }
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

    private func startReader() {
        let thread = Thread { [weak self] in
            self?.readLoop()
        }
        thread.name = "dev.bolted.sync.wire-reader"
        thread.start()
    }

    private func readLoop() {
        let decoder = JSONDecoder()
        var buffer = Data()
        while true {
            var chunk = [UInt8](repeating: 0, count: 4096)
            let n = read(fd, &chunk, chunk.count)
            if n <= 0 { break }
            buffer.append(contentsOf: chunk[0..<n])
            while let idx = buffer.firstIndex(of: 0x0A) {
                let line = Data(buffer.prefix(upTo: idx))
                buffer.removeSubrange(...idx)
                guard let env = try? decoder.decode(ServerEnv.self, from: line) else { continue }
                switch env.kind {
                case "response":
                    stateLock.lock()
                    let entry = env.re.flatMap { pending[$0] }
                    if let (sem, slot) = entry {
                        slot.pointee = env.resp
                        sem.signal()
                    }
                    stateLock.unlock()
                case "push":
                    if let p = env.push {
                        callbackQueue.async { [weak self] in self?.onPush?(p) }
                    }
                default:
                    // "refused" for an in-flight seq: fail that requester (nil), keep reading.
                    stateLock.lock()
                    let entry = env.re.flatMap { pending[$0] }
                    if let (sem, _) = entry { sem.signal() }
                    stateLock.unlock()
                }
            }
        }
        // EOF or error: fail everything in flight, then announce.
        stateLock.lock()
        closed = true
        let waiters = pending.values.map { $0.0 }
        pending.removeAll()
        stateLock.unlock()
        Darwin.close(fd)
        waiters.forEach { $0.signal() }
        callbackQueue.async { [weak self] in self?.onDisconnect?() }
    }
}
