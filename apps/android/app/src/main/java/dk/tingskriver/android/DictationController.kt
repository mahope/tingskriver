package dk.tingskriver.android

import dk.tingskriver.android.audio.RecordingSession
import dk.tingskriver.android.speech.SpeechResult

/** Main-thread owner of the insertion associated with a recording. */
class DictationController(
    private val recording: RecordingSession,
    private val insertion: InsertionSession
) {
    private var token: InsertionSession.Session? = null
    val hasInsertion: Boolean get() = token != null

    fun <T> start(inputType: Int, connection: T?, commit: (T, String) -> Boolean, permitted: Boolean): Long? {
        token = insertion.start(inputType, connection?.let { original ->
            { text -> commit(original, text) }
        })
        if (token == null) return null
        return recording.start(permitted).also { if (it == null) invalidate() }
    }

    fun onCaptureFinished(stop: () -> Unit) {
        when (recording.state) {
            RecordingSession.State.STOPPED -> stop()
            RecordingSession.State.ERROR, RecordingSession.State.CANCELLED -> invalidate()
            else -> Unit
        }
    }

    fun stop(transcribe: () -> SpeechResult): InsertionOutcome? {
        recording.stop()
        val active = token ?: return null
        token = null
        return insertion.complete(active, transcribe())
    }

    fun invalidate() {
        insertion.invalidate()
        token = null
    }
}
