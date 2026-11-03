package dk.tingskriver.android

import android.inputmethodservice.InputMethodService
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.Button
import android.widget.TextView
import dk.tingskriver.android.speech.SpeechResult
import dk.tingskriver.android.speech.StubSpeechToText

class TingskriverInputMethodService : InputMethodService() {
    private val speech = StubSpeechToText()
    private var inputView: View? = null
    private var dictationAllowed = false

    override fun onEvaluateFullscreenMode(): Boolean = false

    override fun onCreateInputView(): View {
        val view = layoutInflater.inflate(R.layout.input_view, null)
        inputView = view
        view.findViewById<Button>(R.id.microphone).setOnClickListener {
            if (dictationAllowed) {
                // No audio capture or InputConnection writes in this skeleton.
                when (speech.transcribe(shortArrayOf(), 16_000)) {
                    SpeechResult.NotImplemented -> renderStatus()
                    else -> error("The skeleton must only use StubSpeechToText")
                }
            }
        }
        view.findViewById<Button>(R.id.switch_keyboard).setOnClickListener {
            getSystemService(InputMethodManager::class.java).showInputMethodPicker()
        }
        renderStatus()
        return view
    }

    override fun onStartInput(attribute: EditorInfo?, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        dictationAllowed = attribute?.let { InputPolicy.allowsDictation(it.inputType) } ?: false
        renderStatus()
    }

    override fun onFinishInput() {
        dictationAllowed = false
        renderStatus()
        super.onFinishInput()
    }

    override fun onDestroy() {
        inputView = null
        super.onDestroy()
    }

    private fun renderStatus() {
        inputView?.findViewById<Button>(R.id.microphone)?.isEnabled = dictationAllowed
        inputView?.findViewById<TextView>(R.id.status)?.setText(
            if (dictationAllowed) R.string.stub_status else R.string.unsupported_field
        )
    }
}
