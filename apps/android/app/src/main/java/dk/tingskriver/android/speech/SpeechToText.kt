package dk.tingskriver.android.speech

/** Implementations must run off the UI thread and must not persist PCM or text. */
interface SpeechToText {
    fun transcribe(pcm: ShortArray, sampleRateHz: Int): SpeechResult
}

sealed interface SpeechResult {
    data class Text(val value: String) : SpeechResult
    data object NoSpeech : SpeechResult
    data object Cancelled : SpeechResult
    data object NotImplemented : SpeechResult
    data class Failure(val message: String) : SpeechResult
}

/** Deliberately does not capture audio, contact a provider or fabricate text. */
class StubSpeechToText : SpeechToText {
    override fun transcribe(pcm: ShortArray, sampleRateHz: Int): SpeechResult =
        SpeechResult.NotImplemented
}
