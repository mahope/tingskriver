package dk.tingskriver.android

import dk.tingskriver.android.speech.SpeechResult
import org.junit.Assert.*
import org.junit.Test

class InsertionSessionTest {
    private val sessions = InsertionSession()
    private val writes = mutableListOf<String>()
    private fun start() = sessions.start(0x1) { writes.add(it); true }!!

    @Test fun lateResultCannotWriteToAnotherConnection() {
        val oldWrites = mutableListOf<String>()
        val newWrites = mutableListOf<String>()
        val old = sessions.start(0x1) { oldWrites.add(it); true }!!
        sessions.invalidate()
        val current = sessions.start(0x1) { newWrites.add(it); true }!!
        assertEquals(InsertionOutcome.Ignored, sessions.complete(old, SpeechResult.Text("Gammel")))
        assertEquals(InsertionOutcome.Inserted, sessions.complete(current, SpeechResult.Text("Ny")))
        assertTrue(oldWrites.isEmpty())
        assertEquals(listOf("Ny"), newWrites)
    }

    @Test fun reentrantCompletionCannotCommitTwice() {
        lateinit var token: InsertionSession.Session
        token = sessions.start(0x1) {
            writes.add(it)
            assertEquals(InsertionOutcome.Ignored, sessions.complete(token, SpeechResult.Text("Igen")))
            true
        }!!
        assertEquals(InsertionOutcome.Inserted, sessions.complete(token, SpeechResult.Text("Hej")))
        assertEquals(listOf("Hej"), writes)
    }

    @Test fun commitsOriginalTargetOnlyOnce() {
        val session = start()
        assertEquals(InsertionOutcome.Inserted, sessions.complete(session, SpeechResult.Text("Hej")))
        assertEquals(InsertionOutcome.Ignored, sessions.complete(session, SpeechResult.Text("Igen")))
        assertEquals(listOf("Hej"), writes)
    }

    @Test fun fieldAndAppChangesInvalidateEvenWhenConnectionIsReused() {
        repeat(2) {
            val old = start()
            sessions.invalidate()
            val next = start()
            assertEquals(InsertionOutcome.Ignored, sessions.complete(old, SpeechResult.Text("Gammel")))
            assertTrue(writes.isEmpty())
            sessions.complete(next, SpeechResult.Cancelled)
        }
    }

    @Test fun finishHideAndDestroyInvalidateCallbacks() {
        repeat(3) {
            val session = start()
            sessions.invalidate()
            assertEquals(InsertionOutcome.Ignored, sessions.complete(session, SpeechResult.Text("Sent")))
        }
        assertTrue(writes.isEmpty())
    }

    @Test fun cancellationAndNewSessionRejectOldCallback() {
        val old = start()
        sessions.complete(old, SpeechResult.Cancelled)
        val next = start()
        sessions.complete(old, SpeechResult.Text("Sent"))
        sessions.complete(next, SpeechResult.Text("Ny"))
        assertEquals(listOf("Ny"), writes)
    }

    @Test fun emptyAndFailedResultsNeverWriteOrAllowRetry() {
        for (result in listOf(SpeechResult.Text(" \n"), SpeechResult.NoSpeech,
            SpeechResult.Failure("private detail"), SpeechResult.NotImplemented, SpeechResult.Cancelled)) {
            val session = start()
            sessions.complete(session, result)
            sessions.complete(session, SpeechResult.Text("Retry"))
        }
        assertTrue(writes.isEmpty())
    }

    @Test fun missingRejectedAndThrowingConnectionsFailClosed() {
        assertNull(sessions.start(0x1, null))
        for (target in listOf<(String) -> Boolean>({ false }, { throw IllegalStateException() })) {
            val session = sessions.start(0x1, target)!!
            assertEquals(InsertionOutcome.ConnectionError, sessions.complete(session, SpeechResult.Text("Hej")))
            assertEquals(InsertionOutcome.Ignored, sessions.complete(session, SpeechResult.Text("Retry")))
        }
    }

    @Test fun blockedFieldsCannotStartAndInvalidatePreviousSession() {
        for (type in listOf(0, 0x2, 0x12, 0x3, 0x4, 0x81, 0x91, 0xe1, 0x80081)) {
            val old = start()
            assertNull(sessions.start(type) { fail("Blocked field"); true })
            sessions.complete(old, SpeechResult.Text("Sent"))
        }
        assertTrue(writes.isEmpty())
    }
}
