package dk.tingskriver.android

import dk.tingskriver.android.audio.PcmSource
import dk.tingskriver.android.audio.RecordingSession
import dk.tingskriver.android.speech.SpeechResult
import org.junit.Assert.*
import org.junit.Test

class DictationControllerTest {
    private class Connection {
        val writes = mutableListOf<String>()
        fun commit(text: String): Boolean { writes.add(text); return true }
    }

    private class Source : PcmSource {
        var failure = false
        override fun read(target: ShortArray): Int {
            if (failure) throw SecurityException()
            target.fill(7)
            return target.size
        }
        override fun close() {}
    }

    @Test fun completionUsesConnectionCapturedAtStart() {
        val session = RecordingSession { Source() }
        val controller = DictationController(session, InsertionSession())
        val original = Connection()
        var current = original
        assertNotNull(controller.start(1, current, Connection::commit, true))
        current = Connection()
        assertEquals(InsertionOutcome.Inserted, controller.stop { SpeechResult.Text("Hej") })
        assertEquals(listOf("Hej"), original.writes)
        assertTrue(current.writes.isEmpty())
        assertNull(controller.stop { fail("Must not transcribe twice"); SpeechResult.NoSpeech })
    }

    @Test fun failedRecordingStartClearsInsertionSession() {
        var failOpen = true
        val session = RecordingSession {
            if (failOpen) throw SecurityException()
            Source()
        }
        val controller = DictationController(session, InsertionSession())
        val connection = Connection()
        assertNull(controller.start(1, connection, Connection::commit, true))
        assertFalse(controller.hasInsertion)
        assertNull(controller.stop { fail("Must not transcribe failed start"); SpeechResult.NoSpeech })
        assertTrue(connection.writes.isEmpty())
        failOpen = false
        val next = Connection()
        assertNotNull(controller.start(1, next, Connection::commit, true))
        assertEquals(InsertionOutcome.Inserted, controller.stop { SpeechResult.Text("Ny") })
        assertTrue(connection.writes.isEmpty())
        assertEquals(listOf("Ny"), next.writes)
    }

    @Test fun missingConnectionBlockedFieldAndDeniedPermissionCannotLeaveInsertion() {
        val session = RecordingSession { error("Must not open microphone") }
        val controller = DictationController(session, InsertionSession())
        assertNull(controller.start(1, null as Connection?, Connection::commit, true))
        assertFalse(controller.hasInsertion)
        assertNull(controller.start(0x81, Connection(), Connection::commit, true))
        assertFalse(controller.hasInsertion)
        assertNull(controller.start(1, Connection(), Connection::commit, false))
        assertFalse(controller.hasInsertion)
    }

    @Test fun elapsedTimeAndFullBufferCompleteExactlyOnce() {
        for (elapsed in listOf(true, false)) {
            var now = 0L
            val session = RecordingSession({ now }) { Source() }
            val controller = DictationController(session, InsertionSession())
            val connection = Connection()
            val token = controller.start(1, connection, Connection::commit, true)!!
            if (elapsed) now = 30_000
            repeat(if (elapsed) 1 else 750) { session.capture(token) }
            assertEquals(RecordingSession.State.STOPPED, session.state)
            var transcriptions = 0
            repeat(2) {
                controller.onCaptureFinished {
                    controller.stop { transcriptions++; SpeechResult.Text("Hej") }
                }
            }
            assertEquals(1, transcriptions)
            assertEquals(listOf("Hej"), connection.writes)
            assertFalse(controller.hasInsertion)
        }
    }

    @Test fun errorsAndCancellationDiscardInsertion() {
        for (cancelled in listOf(true, false)) {
            val source = Source()
            val session = RecordingSession { source }
            val controller = DictationController(session, InsertionSession())
            val connection = Connection()
            val token = controller.start(1, connection, Connection::commit, true)!!
            if (cancelled) session.cancel() else {
                source.failure = true
                session.capture(token)
            }
            controller.onCaptureFinished { fail("Must not finish cancelled or failed capture") }
            assertFalse(controller.hasInsertion)
            assertNull(controller.stop { fail("Must not transcribe"); SpeechResult.NoSpeech })
            assertTrue(connection.writes.isEmpty())
        }
    }
}
