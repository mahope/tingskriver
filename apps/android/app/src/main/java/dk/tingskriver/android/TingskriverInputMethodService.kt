package dk.tingskriver.android

import android.Manifest
import android.app.KeyguardManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.inputmethodservice.InputMethodService
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.Button
import android.widget.TextView
import dk.tingskriver.android.audio.AndroidPcmSource
import dk.tingskriver.android.audio.RecordingSession

class TingskriverInputMethodService : InputMethodService() {
    private val session = RecordingSession { AndroidPcmSource(this) }
    private val handler = Handler(Looper.getMainLooper())
    private var worker: Thread? = null
    private var inputView: View? = null
    private var dictationAllowed = false
    private var deadline = 0L
    private val screenOff = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) { cancelRecording() }
    }
    private val monitor = object : Runnable {
        override fun run() {
            if (session.state != RecordingSession.State.RECORDING) {
                renderStatus()
                return
            }
            if (!canRecord()) cancelRecording()
            else if (SystemClock.elapsedRealtime() >= deadline) stopRecording()
            else handler.postDelayed(this, 100)
        }
    }

    override fun onCreate() {
        super.onCreate()
        registerReceiver(screenOff, IntentFilter(Intent.ACTION_SCREEN_OFF))
    }

    override fun onEvaluateFullscreenMode(): Boolean = false

    override fun onCreateInputView(): View {
        cancelRecording()
        val view = layoutInflater.inflate(R.layout.input_view, null)
        inputView = view
        view.findViewById<Button>(R.id.microphone).setOnClickListener {
            if (session.state == RecordingSession.State.RECORDING) stopRecording()
            else if (canRecord()) {
                finishWorker()
                val token = session.start(hasPermission())
                if (token != null) {
                    deadline = SystemClock.elapsedRealtime() + 30_000
                    worker = Thread({
                        try {
                            while (session.capture(token)) Thread.sleep(20)
                        } catch (_: InterruptedException) {
                            // Lifecycle owner has already released the microphone.
                        }
                    }, "tingskriver-pcm").also { it.start() }
                    handler.post(monitor)
                }
            }
            renderStatus()
        }
        view.findViewById<Button>(R.id.cancel_recording).setOnClickListener { cancelRecording() }
        view.findViewById<Button>(R.id.switch_keyboard).setOnClickListener {
            cancelRecording()
            getSystemService(InputMethodManager::class.java).showInputMethodPicker()
        }
        renderStatus()
        return view
    }

    override fun onStartInput(attribute: EditorInfo?, restarting: Boolean) {
        cancelRecording()
        super.onStartInput(attribute, restarting)
        dictationAllowed = attribute?.let { InputPolicy.allowsDictation(it.inputType) } ?: false
        renderStatus()
    }

    override fun onWindowShown() {
        super.onWindowShown()
        renderStatus()
    }

    override fun onWindowHidden() {
        cancelRecording()
        super.onWindowHidden()
    }

    override fun onFinishInputView(finishingInput: Boolean) {
        cancelRecording()
        super.onFinishInputView(finishingInput)
    }
    override fun onFinishInput() {
        cancelRecording()
        dictationAllowed = false
        renderStatus()
        super.onFinishInput()
    }
    override fun onDestroy() {
        cancelRecording()
        unregisterReceiver(screenOff)
        inputView = null
        super.onDestroy()
    }

    private fun hasPermission() = checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
    private fun canRecord() = dictationAllowed && isInputViewShown && hasPermission() &&
        !getSystemService(KeyguardManager::class.java).isKeyguardLocked

    private fun stopRecording() {
        session.stop()
        finishWorker()
    }
    private fun cancelRecording() {
        session.cancel()
        finishWorker()
    }
    private fun finishWorker() {
        handler.removeCallbacks(monitor)
        worker?.interrupt()
        worker = null
        renderStatus()
    }

    private fun renderStatus() {
        val recording = session.state == RecordingSession.State.RECORDING
        inputView?.findViewById<Button>(R.id.microphone)?.apply {
            isEnabled = dictationAllowed && hasPermission()
            setText(if (recording) R.string.stop_recording else R.string.microphone)
        }
        inputView?.findViewById<Button>(R.id.cancel_recording)?.isEnabled = recording
        inputView?.findViewById<TextView>(R.id.status)?.setText(when {
            !dictationAllowed -> R.string.unsupported_field
            !hasPermission() -> R.string.permission_needed
            recording -> R.string.recording_status
            session.state == RecordingSession.State.ERROR -> R.string.recording_error
            session.state == RecordingSession.State.STOPPED -> R.string.stopped_status
            session.state == RecordingSession.State.CANCELLED -> R.string.cancelled_status
            else -> R.string.ready_status
        })
    }
}
