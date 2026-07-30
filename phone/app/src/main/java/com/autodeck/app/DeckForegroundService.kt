package com.autodeck.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Binder
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import org.json.JSONObject

class DeckForegroundService : Service() {

    interface Listener {
        fun onLayout(json: JSONObject)
        fun onVolumeSync(volume: Float)
        fun onStatus(text: String)
    }

    inner class LocalBinder : Binder() {
        fun service(): DeckForegroundService = this@DeckForegroundService
    }

    private val binder = LocalBinder()
    private var listener: Listener? = null
    private var lastLayout: JSONObject? = null
    private var lastVolume: Float? = null
    private var lastStatus: String = "Searching for PC..."

    lateinit var deckClient: DeckClient
        private set
    lateinit var discovery: DeviceDiscovery
        private set

    private var wifiLock: WifiManager.WifiLock? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var connectedOnce = false
    private var currentDeviceName: String? = null

    private val mainHandler = Handler(Looper.getMainLooper())
    private var searchTimeoutRunnable: Runnable? = null

    private val prefs by lazy { getSharedPreferences("autodeck", MODE_PRIVATE) }

    override fun onCreate() {
        super.onCreate()

        deckClient = DeckClient(
            context = applicationContext,
            onLayout = { json ->
                lastLayout = json
                listener?.onLayout(json)
            },
            onVolumeSync = { volume ->
                lastVolume = volume
                listener?.onVolumeSync(volume)
            },
            onStatus = { text ->
                val name = currentDeviceName
                // "연결 끊김"만 보면 왜 끊겼는지 알 길이 없어서, 어느 PC와의
                // 연결인지 이름을 붙여 원인 파악에 도움이 되게 한다.
                val display = if (text == "Disconnected" && name != null) {
                    "Lost connection to $name. Retrying…"
                } else {
                    text
                }
                lastStatus = display
                listener?.onStatus(display)
                updateNotification(display)
            }
        )

        // 폰 하나는 PC 하나에만 연결한다 - 여러 PC를 오가면 오히려 헷갈려서
        // 1:1 연결만 지원한다. 저장된 PC가 다시 보이면 그걸로, 처음이면 맨 처음
        // 찾은 PC로 연결하고 그 이후로 찾히는 다른 PC는 전부 무시한다.
        discovery = DeviceDiscovery(
            context = this,
            onDeviceFound = { name, host, port ->
                if (currentDeviceName != name) {
                    val preferred = prefs.getString(KEY_LAST_DEVICE, null)
                    val shouldConnect = !connectedOnce || (preferred != null && name == preferred)
                    if (shouldConnect) {
                        connectedOnce = true
                        currentDeviceName = name
                        prefs.edit().putString(KEY_LAST_DEVICE, name).apply()
                        deckClient.connect("ws://$host:$port/ws")
                    }
                }
            },
            onDeviceLost = { name ->
                if (currentDeviceName == name) {
                    currentDeviceName = null
                }
            }
        )

        acquireLocks()
        ServiceCompat.startForeground(
            this,
            NOTIFICATION_ID,
            buildNotification(lastStatus),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
        )
        discovery.start()
        scheduleSearchTimeout()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder = binder

    fun setListener(l: Listener?) {
        listener = l
        lastLayout?.let { l?.onLayout(it) }
        lastVolume?.let { l?.onVolumeSync(it) }
        l?.onStatus(lastStatus)
    }

    fun currentStatus(): String = lastStatus

    fun rescan() {
        connectedOnce = false
        currentDeviceName = null
        deckClient.disconnect()
        lastStatus = "Searching for PC..."
        listener?.onStatus(lastStatus)
        updateNotification(lastStatus)
        scheduleSearchTimeout()
        discovery.start()
    }

    private fun scheduleSearchTimeout() {
        searchTimeoutRunnable?.let { mainHandler.removeCallbacks(it) }
        val r = Runnable {
            if (!connectedOnce) {
                lastStatus = "No PC found on this network"
                listener?.onStatus(lastStatus)
                updateNotification(lastStatus)
            }
        }
        searchTimeoutRunnable = r
        mainHandler.postDelayed(r, 8000)
    }

    fun currentSsid(): String? {
        val wm = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        val ssid = wm.connectionInfo?.ssid ?: return null
        val cleaned = ssid.trim('"')
        if (cleaned.isEmpty() || cleaned == "<unknown ssid>") return null
        return cleaned
    }

    private fun acquireLocks() {
        val wm = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        wifiLock = wm.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "autodeck:wifi").apply {
            setReferenceCounted(false)
            acquire()
        }
        val pm = applicationContext.getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "autodeck:wake").apply {
            setReferenceCounted(false)
            acquire()
        }
    }

    private fun ensureChannel(): String {
        val channelId = "autodeck_status"
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (manager.getNotificationChannel(channelId) == null) {
            val channel = NotificationChannel(
                channelId,
                "AutoDeck Connection Status",
                NotificationManager.IMPORTANCE_LOW
            )
            manager.createNotificationChannel(channel)
        }
        return channelId
    }

    private fun buildNotification(status: String): Notification {
        val channelId = ensureChannel()
        return NotificationCompat.Builder(this, channelId)
            .setContentTitle("AutoDeck")
            .setContentText(status)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setOngoing(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    private fun updateNotification(status: String) {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.notify(NOTIFICATION_ID, buildNotification(status))
    }

    override fun onDestroy() {
        searchTimeoutRunnable?.let { mainHandler.removeCallbacks(it) }
        discovery.stop()
        deckClient.disconnect()
        wifiLock?.let { if (it.isHeld) it.release() }
        wakeLock?.let { if (it.isHeld) it.release() }
        super.onDestroy()
    }

    companion object {
        const val NOTIFICATION_ID = 1
        private const val KEY_LAST_DEVICE = "last_device_name"
    }
}
