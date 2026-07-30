package com.autodeck.app

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.os.PowerManager
import android.provider.Settings
import android.view.View
import android.view.WindowManager
import android.widget.Button
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import org.json.JSONObject

class MainActivity : AppCompatActivity(), DeckForegroundService.Listener {

    private var radialView: RadialDeckView? = null
    private var settingsPage: View? = null
    private var statusText: TextView? = null
    private var ssidValue: TextView? = null
    private var rescanButton: Button? = null

    private var lastLayoutJson: JSONObject? = null

    private var service: DeckForegroundService? = null
    private var bound = false

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
            val local = binder as DeckForegroundService.LocalBinder
            service = local.service()
            bound = true
            service?.setListener(this@MainActivity)
            updateSsidDisplay()
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            service = null
            bound = false
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        applyImmersiveFullscreen()
        setContentView(R.layout.activity_main)

        bindGridPage()
        bindSettingsPage()

        val intent = Intent(this, DeckForegroundService::class.java)
        ContextCompat.startForegroundService(this, intent)
        bindService(intent, connection, Context.BIND_AUTO_CREATE)

        if (android.os.Build.VERSION.SDK_INT >= 33) {
            if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQ_NOTIFICATIONS)
            }
        }

        requestBatteryOptimizationExemption()
    }

    private fun applyImmersiveFullscreen() {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        val controller = WindowInsetsControllerCompat(window, window.decorView)
        controller.hide(WindowInsetsCompat.Type.systemBars())
        controller.systemBarsBehavior =
            WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE

        val cutoutMode = if (Build.VERSION.SDK_INT >= 30) {
            WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS
        } else {
            WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
        }
        window.attributes = window.attributes.apply { layoutInDisplayCutoutMode = cutoutMode }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) applyImmersiveFullscreen()
    }

    override fun onBackPressed() {
        if (settingsPage?.visibility == View.VISIBLE) {
            showGrid()
        } else {
            super.onBackPressed()
        }
    }

    private fun requestBatteryOptimizationExemption() {
        val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
        if (!pm.isIgnoringBatteryOptimizations(packageName)) {
            val intent = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
                data = Uri.parse("package:$packageName")
            }
            try {
                startActivity(intent)
            } catch (_: Exception) {
            }
        }
    }

    override fun onDestroy() {
        if (bound) {
            service?.setListener(null)
            unbindService(connection)
            bound = false
        }
        super.onDestroy()
    }

    override fun onLayout(json: JSONObject) {
        lastLayoutJson = json
        runOnUiThread { applyLayout(json) }
    }

    override fun onVolumeSync(volume: Float) {
        runOnUiThread { radialView?.updateVolumeBaseline(volume) }
    }

    override fun onStatus(text: String) {
        runOnUiThread { statusText?.text = text }
    }

    private fun applyLayout(json: JSONObject) {
        val arr = json.getJSONArray("buttons")
        val buttons = ArrayList<DeckButton>(arr.length())
        for (i in 0 until arr.length()) {
            val b = arr.getJSONObject(i)
            val id = b.getString("id")
            val label = if (b.has("label")) b.getString("label") else null
            val icon = if (b.has("icon")) b.getString("icon") else null
            buttons.add(DeckButton(id, label, icon))
        }
        radialView?.updateButtons(buttons)

        if (json.has("volume")) {
            radialView?.updateVolumeBaseline(json.getDouble("volume").toFloat())
        }
    }

    private fun bindGridPage() {
        val rv = findViewById<RadialDeckView>(R.id.radial_deck)
        radialView = rv
        rv.onPress = { id -> service?.deckClient?.press(id) }
        rv.onVolumeChange = { level -> service?.deckClient?.setVolume(level) }
        rv.onSettingsPress = { showSettings() }
        lastLayoutJson?.let { applyLayout(it) }
    }

    private fun bindSettingsPage() {
        val page = findViewById<View>(R.id.page_settings)
        settingsPage = page
        statusText = page.findViewById(R.id.status)
        ssidValue = page.findViewById(R.id.ssid_value)
        rescanButton = page.findViewById(R.id.rescan_button)

        statusText?.text = service?.currentStatus() ?: "Not connected"

        page.findViewById<Button>(R.id.back_button).setOnClickListener { showGrid() }

        rescanButton?.setOnClickListener {
            service?.rescan()
        }

        page.findViewById<Button>(R.id.display_settings_button).setOnClickListener {
            try {
                startActivity(Intent(Settings.ACTION_DISPLAY_SETTINGS))
            } catch (_: Exception) {
            }
        }

        page.findViewById<Button>(R.id.battery_settings_button).setOnClickListener {
            requestBatteryOptimizationExemption()
        }

        updateSsidDisplay()
    }

    private fun showSettings() {
        radialView?.visibility = View.GONE
        settingsPage?.visibility = View.VISIBLE
    }

    private fun showGrid() {
        settingsPage?.visibility = View.GONE
        radialView?.visibility = View.VISIBLE
    }

    private fun updateSsidDisplay() {
        val hasPermission = checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) ==
            PackageManager.PERMISSION_GRANTED
        if (!hasPermission) {
            requestPermissions(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION), REQ_LOCATION)
            ssidValue?.text = "Checking..."
            return
        }
        ssidValue?.text = service?.currentSsid() ?: "Unknown"
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQ_LOCATION) {
            updateSsidDisplay()
        }
    }

    companion object {
        private const val REQ_LOCATION = 1
        private const val REQ_NOTIFICATIONS = 2
    }
}
