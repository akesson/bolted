---
name: art-gc-probes-need-a-control
description: "Proving an object is (not) collected on Android ART needs a ReferenceQueue and a worker thread — WeakReference.get() in a polling loop silently keeps the object alive"
metadata:
  node_type: memory
  type: feedback
---

Any Android/ART test that concludes "object X was (not) garbage collected" is wrong by default
unless it does all three of these. Step 05 got a *false* H1 confirmation before they were in place:

1. **Never call `weak.get()` in the GC-polling loop.** ART's concurrent-copying collector puts a
   **read barrier** on `Reference.get()`: reading the referent marks it reachable for the cycle in
   progress. A `get()`-then-`System.gc()` loop keeps the object alive forever — it made an abandoned
   `ByteArray` look uncollectable. Detect collection by polling a **`ReferenceQueue`**.
2. **Create the referent on a worker thread and `join()` it.** An instrumented APK is `debuggable`,
   so ART treats every dex register of a live frame as a GC root; a dead local still pins the object.
3. **Keep a permanent control test** that an abandoned plain `ByteArray` *is* collectable. Without it
   a "not collected" result is indistinguishable from "the GC never ran", and the whole probe is
   unfalsifiable.

**Why:** two independent mechanisms produce confident, plausible, wrong answers, and the failing
direction is the one that confirms most hypotheses ("Rust never freed it!").

**How to apply:** see `android/profile-probe/.../LifecycleProbe.kt` (`gc_control_aPlainObjectIsCollectable`,
`abandonOnAnotherThread`, `awaitCollection`). Same reflex as
[[bolted-verify-in-a-real-browser]]: a green assertion is not evidence until the control passes.
