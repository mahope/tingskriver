package dk.tingskriver.android

import dk.tingskriver.android.audio.PcmSource
import dk.tingskriver.android.audio.RecordingSession
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class RecordingSessionTest {
    private class Source : PcmSource {
        var closed = 0
        var failure = false
        var count = 640
        override fun read(target: ShortArray): Int {
            if (failure) throw SecurityException()
            target.fill(7)
            return count
        }
        override fun close() { closed++ }
    }

    @Test fun lifecycleCancellationBeforeRecordingKeepsReadyState() {
        val session = RecordingSession { error("Must not open microphone") }
        session.cancel()
        assertEquals(RecordingSession.State.IDLE, session.state)
        assertFalse(session.hasBuffer)
    }

    @Test fun capsAtThirtySecondsAndReleasesExactlyOnce() {
        val source = Source()
        val session = RecordingSession { source }
        val generation = session.start(true)!!
        repeat(750) { session.capture(generation) }
        assertEquals(480_000, session.capturedSamples)
        assertEquals(RecordingSession.State.STOPPED, session.state)
        assertEquals(1, source.closed)
        assertFalse(session.hasBuffer)
        session.stop()
        assertEquals(1, source.closed)
    }

    @Test fun deniedPermissionNeverCreatesSource() {
        val session = RecordingSession { error("Must not open microphone") }
        assertNull(session.start(false))
        assertEquals(RecordingSession.State.ERROR, session.state)
        assertFalse(session.hasBuffer)
    }

    @Test fun cancellationInvalidatesWorkerAndAllowsRestart() {
        val sources = mutableListOf<Source>()
        val session = RecordingSession { Source().also { sources.add(it) } }
        val old = session.start(true)!!
        assertNull(session.start(true))
        session.cancel()
        val current = session.start(true)!!
        assertFalse(session.capture(old))
        assertTrue(session.capture(current))
        assertEquals(640, session.capturedSamples)
        session.cancel()
        assertEquals(listOf(1, 1), sources.map { it.closed })
        assertEquals(RecordingSession.State.CANCELLED, session.state)
        assertFalse(session.hasBuffer)
    }

    @Test fun revokedPermissionAndReadErrorsReleaseAndDiscard() {
        val source = Source()
        val session = RecordingSession { source }
        val generation = session.start(true)!!
        source.failure = true
        assertFalse(session.capture(generation))
        assertEquals(RecordingSession.State.ERROR, session.state)
        assertEquals(1, source.closed)
        assertFalse(session.hasBuffer)
    }

    @Test fun wallClockLimitStopsEvenWhenMicrophoneReturnsNoSamples() {
        var now = 0L
        val source = Source().also { it.count = 0 }
        val session = RecordingSession({ now }) { source }
        val token = session.start(true)!!
        now = 29_999
        assertTrue(session.capture(token))
        now = 30_000
        assertFalse(session.capture(token))
        assertEquals(RecordingSession.State.STOPPED, session.state)
        assertEquals(0, session.capturedSamples)
        assertEquals(1, source.closed)
    }

    @Test fun partialFinalChunkNeverExceedsCapacity() {
        val source = Source().also { it.count = 637 }
        val session = RecordingSession { source }
        val token = session.start(true)!!
        repeat(754) { session.capture(token) }
        assertEquals(480_000, session.capturedSamples)
        assertEquals(1, source.closed)
        assertFalse(session.hasBuffer)
    }

    @Test fun negativeReadResultIsAnError() {
        val source = Source().also { it.count = -6 }
        val session = RecordingSession { source }
        val token = session.start(true)!!
        assertFalse(session.capture(token))
        assertEquals(RecordingSession.State.ERROR, session.state)
        assertEquals(1, source.closed)
        assertFalse(session.hasBuffer)
    }

    @Test fun failedOpenAndManualStopAreSafe() {
        val failed = RecordingSession { throw SecurityException() }
        assertNull(failed.start(true))
        assertEquals(RecordingSession.State.ERROR, failed.state)
        val source = Source()
        val session = RecordingSession { source }
        session.start(true)
        session.stop()
        assertEquals(RecordingSession.State.STOPPED, session.state)
        assertFalse(session.hasBuffer)
        assertEquals(1, source.closed)
    }
}
