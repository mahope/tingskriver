package dk.tingskriver.android

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.os.Build
import android.provider.Settings
import android.view.View
import android.view.WindowInsets
import android.view.inputmethod.InputMethodManager
import android.widget.Button
import android.widget.TextView

class SettingsActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.settings)
        // targetSdk 35+ draws edge-to-edge; keep controls outside system bars.
        findViewById<View>(android.R.id.content).setOnApplyWindowInsetsListener { view, insets ->
            applySystemBarPadding(view, insets)
            insets
        }
        findViewById<Button>(R.id.allow_microphone).setOnClickListener {
            requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 1)
        }
        findViewById<Button>(R.id.enable_keyboard).setOnClickListener {
            startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS))
        }
        findViewById<Button>(R.id.switch_keyboard).setOnClickListener {
            getSystemService(InputMethodManager::class.java).showInputMethodPicker()
        }
    }

    override fun onResume() {
        super.onResume()
        renderPermission()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        renderPermission()
    }

    private fun renderPermission() {
        val granted = checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        findViewById<TextView>(R.id.permission_status).setText(
            if (granted) R.string.permission_granted else R.string.permission_needed
        )
        findViewById<Button>(R.id.allow_microphone).isEnabled = !granted
    }

    private fun applySystemBarPadding(view: View, insets: WindowInsets) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
        } else {
            applyLegacySystemBarPadding(view, insets)
        }
    }

    @Suppress("DEPRECATION") // Required on supported Android 8–10 devices.
    private fun applyLegacySystemBarPadding(view: View, insets: WindowInsets) {
        view.setPadding(
            insets.systemWindowInsetLeft,
            insets.systemWindowInsetTop,
            insets.systemWindowInsetRight,
            insets.systemWindowInsetBottom
        )
    }
}
