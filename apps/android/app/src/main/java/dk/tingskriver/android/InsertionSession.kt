package dk.tingskriver.android

import dk.tingskriver.android.speech.SpeechResult

enum class InsertionOutcome { Inserted, Ignored, ConnectionError, SpeechError, NotImplemented }

/** Confined to the IME main thread, including completion callbacks from future workers. */
class InsertionSession {
    class Session internal constructor(internal val generation: Long)

    private var generation = 0L
    private var active: Session? = null
    private var commit: ((String) -> Boolean)? = null

    fun invalidate() {
        generation++
        active = null
        commit = null
    }

    fun start(inputType: Int, originalConnection: ((String) -> Boolean)?): Session? {
        invalidate()
        if (!InputPolicy.allowsDictation(inputType) || originalConnection == null) return null
        return Session(generation).also {
            active = it
            commit = originalConnection
        }
    }

    fun complete(session: Session, result: SpeechResult): InsertionOutcome {
        if (active !== session || session.generation != generation) return InsertionOutcome.Ignored
        val originalConnection = commit
        // Consume before calling editor code: rejection or reentrant callbacks must not retry.
        invalidate()
        return when (result) {
            is SpeechResult.Text -> {
                if (result.value.isBlank()) InsertionOutcome.Ignored
                else try {
                    if (originalConnection?.invoke(result.value) == true) InsertionOutcome.Inserted
                    else InsertionOutcome.ConnectionError
                } catch (_: RuntimeException) {
                    InsertionOutcome.ConnectionError
                }
            }
            is SpeechResult.Failure -> InsertionOutcome.SpeechError
            SpeechResult.NotImplemented -> InsertionOutcome.NotImplemented
            SpeechResult.Cancelled, SpeechResult.NoSpeech -> InsertionOutcome.Ignored
        }
    }
}
