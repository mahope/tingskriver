package dk.tingskriver.android

import dk.tingskriver.android.speech.SpeechResult
import dk.tingskriver.android.speech.StubSpeechToText
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test

class StubSpeechToTextTest {
    @Test
    fun emptyAudioDoesNotProduceFabricatedText() {
        assertEquals(SpeechResult.NotImplemented, StubSpeechToText().transcribe(shortArrayOf(), 16_000))
    }

    @Test
    fun suppliedAudioIsUnchangedAndNeverTranscribed() {
        val audio = shortArrayOf(10, -20, 30)
        assertEquals(SpeechResult.NotImplemented, StubSpeechToText().transcribe(audio, 16_000))
        assertArrayEquals(shortArrayOf(10, -20, 30), audio)
    }
}
