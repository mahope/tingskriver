package dk.tingskriver.android

import android.view.inputmethod.EditorInfo
import dk.tingskriver.android.speech.SpeechResult
import org.junit.Assert.*
import org.junit.Test

class InputMethodLifecycleTest {
    private val service = TingskriverInputMethodService()
    private val insertion = service.javaClass.getDeclaredField("insertion").let {
        it.isAccessible = true
        it.get(service) as InsertionSession
    }
    private val oldWrites = mutableListOf<String>()
    private val newWrites = mutableListOf<String>()

    private fun editor(app: String, field: Int) = EditorInfo().apply {
        packageName = app
        fieldId = field
        inputType = 1
    }

    private fun assertInvalidated(event: () -> Unit) {
        val old = insertion.start(1) { oldWrites.add(it); true }!!
        event()
        assertEquals(InsertionOutcome.Ignored, insertion.complete(old, SpeechResult.Text("Sent")))
        assertTrue(oldWrites.isEmpty())
        assertTrue(newWrites.isEmpty())
    }

    @Test fun sameAppFieldChangeRejectsOldResult() {
        service.onStartInput(editor("test.first", 1), false)
        assertInvalidated { service.onStartInput(editor("test.first", 2), false) }
    }

    @Test fun appChangeRejectsOldResult() {
        service.onStartInput(editor("test.first", 1), false)
        assertInvalidated { service.onStartInput(editor("test.second", 1), false) }
    }

    @Test fun restartedInputRejectsOldResult() {
        assertInvalidated { service.onStartInput(editor("test.first", 1), true) }
    }

    @Test fun finishInputRejectsOldResult() = assertInvalidated { service.onFinishInput() }

    @Test fun finishInputViewRejectsOldResult() = assertInvalidated { service.onFinishInputView(false) }

    @Test fun hiddenWindowRejectsOldResult() = assertInvalidated { service.onWindowHidden() }

    @Test fun destroyedServiceRejectsOldResult() = assertInvalidated { service.onDestroy() }

    @Test fun newSessionAfterCancellationRejectsOldResult() {
        val old = insertion.start(1) { oldWrites.add(it); true }!!
        service.onWindowHidden()
        service.onStartInput(editor("test.first", 2), false)
        val next = insertion.start(1) { newWrites.add(it); true }!!
        assertEquals(InsertionOutcome.Ignored, insertion.complete(old, SpeechResult.Text("Sent")))
        assertTrue(oldWrites.isEmpty())
        assertTrue(newWrites.isEmpty())
        assertEquals(InsertionOutcome.Inserted, insertion.complete(next, SpeechResult.Text("Ny")))
        assertEquals(listOf("Ny"), newWrites)
    }
}
