package dk.tingskriver.android.audio

interface PcmSource {
    /** Non-blocking read of mono 16 kHz signed 16-bit samples. */
    fun read(target: ShortArray): Int
    fun close()
}

/** One owner for microphone and RAM. All source access is serialized with cancellation. */
class RecordingSession(
    private val nowMillis: () -> Long = { System.nanoTime() / 1_000_000 },
    private val open: () -> PcmSource
) {
    enum class State { IDLE, RECORDING, STOPPED, CANCELLED, ERROR }
    @Volatile var state = State.IDLE
        private set
    @Volatile var capturedSamples = 0
        private set
    val hasBuffer: Boolean @Synchronized get() = buffer != null
    private var buffer: ShortArray? = null
    private val chunk = ShortArray(640)
    private var source: PcmSource? = null
    private var generation = 0L
    private var deadline = 0L

    @Synchronized fun start(permitted: Boolean): Long? {
        if (state == State.RECORDING) return null
        capturedSamples = 0
        if (!permitted) { finish(State.ERROR); return null }
        return try {
            buffer = ShortArray(16_000 * 30)
            source = open()
            deadline = nowMillis() + 30_000
            state = State.RECORDING
            ++generation
        } catch (_: RuntimeException) {
            finish(State.ERROR)
            null
        }
    }

    @Synchronized fun capture(token: Long): Boolean {
        if (token != generation || state != State.RECORDING) return false
        if (nowMillis() >= deadline) { finish(State.STOPPED); return false }
        try {
            val count = source!!.read(chunk)
            check(count in 0..chunk.size)
            val accepted = minOf(count, buffer!!.size - capturedSamples)
            chunk.copyInto(buffer!!, capturedSamples, 0, accepted)
            capturedSamples += accepted
            chunk.fill(0)
            if (capturedSamples == buffer!!.size) finish(State.STOPPED)
        } catch (_: RuntimeException) {
            finish(State.ERROR)
        }
        return state == State.RECORDING
    }

    @Synchronized fun stop() { if (state == State.RECORDING) finish(State.STOPPED) }
    @Synchronized fun cancel() { if (state == State.RECORDING) finish(State.CANCELLED) }

    private fun finish(next: State) {
        ++generation
        val owned = source
        source = null
        try { owned?.close() } catch (_: RuntimeException) { /* Still discard PCM. */ }
        buffer?.fill(0)
        buffer = null
        chunk.fill(0)
        state = next
    }
}
