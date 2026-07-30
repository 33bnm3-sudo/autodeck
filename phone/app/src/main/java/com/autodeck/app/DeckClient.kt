package com.autodeck.app

import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONObject
import java.net.InetAddress
import java.net.Socket
import java.util.UUID
import java.util.concurrent.TimeUnit
import javax.net.SocketFactory

// OkHttp WebSocket은 기본적으로 Nagle 알고리즘이 켜진 채로 소켓을 만들어서,
// press처럼 작은 메시지도 전송이 지연될 수 있다. 소켓 팩토리를 직접 넘겨
// TCP_NODELAY를 걸어야 즉시 전송된다.
private class NoDelaySocketFactory : SocketFactory() {
    override fun createSocket(): Socket = Socket().apply { tcpNoDelay = true }
    override fun createSocket(host: String?, port: Int): Socket =
        Socket(host, port).apply { tcpNoDelay = true }
    override fun createSocket(host: String?, port: Int, localHost: InetAddress?, localPort: Int): Socket =
        Socket(host, port, localHost, localPort).apply { tcpNoDelay = true }
    override fun createSocket(host: InetAddress?, port: Int): Socket =
        Socket(host, port).apply { tcpNoDelay = true }
    override fun createSocket(
        address: InetAddress?,
        port: Int,
        localAddress: InetAddress?,
        localPort: Int
    ): Socket = Socket(address, port, localAddress, localPort).apply { tcpNoDelay = true }
}

class DeckClient(
    context: Context,
    private val onLayout: (JSONObject) -> Unit,
    private val onVolumeSync: (Float) -> Unit,
    private val onStatus: (String) -> Unit
) {
    private val prefs = context.applicationContext.getSharedPreferences("autodeck", Context.MODE_PRIVATE)

    // 이 폰을 PC가 구분할 수 있는 영구 ID. 최초 1회 생성해 계속 재사용한다 -
    // 이게 있어야 PC가 "이전에 승인한 기기"를 알아보고 매번 다시 물어보지 않는다.
    private val deviceId: String by lazy {
        prefs.getString(KEY_DEVICE_ID, null) ?: UUID.randomUUID().toString().also {
            prefs.edit().putString(KEY_DEVICE_ID, it).apply()
        }
    }

    private var pairingToken: String?
        get() = prefs.getString(KEY_PAIRING_TOKEN, null)
        set(value) { prefs.edit().putString(KEY_PAIRING_TOKEN, value).apply() }

    private val client = OkHttpClient.Builder()
        .pingInterval(15, TimeUnit.SECONDS)
        .socketFactory(NoDelaySocketFactory())
        .build()

    private val mainHandler = Handler(Looper.getMainLooper())
    private val backoffMs = longArrayOf(1000, 2000, 4000, 8000, 10000)

    private var socket: WebSocket? = null
    private var lastUrl: String? = null
    private var reconnectAttempt = 0
    private var wantsConnection = false

    fun connect(url: String) {
        lastUrl = url
        wantsConnection = true
        reconnectAttempt = 0
        openSocket(url)
    }

    private fun openSocket(url: String) {
        socket?.cancel()
        onStatus("Connecting...")
        val request = Request.Builder().url(url).build()
        socket = client.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                reconnectAttempt = 0
                onStatus("Waiting for approval on PC…")
                val hello = JSONObject()
                hello.put("type", "hello")
                hello.put("device_id", deviceId)
                hello.put("device", Build.MODEL)
                pairingToken?.let { hello.put("token", it) }
                webSocket.send(hello.toString())
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                val json = JSONObject(text)
                when (json.optString("type")) {
                    "hello-ack" -> {
                        pairingToken = if (json.has("token") && !json.isNull("token")) {
                            json.optString("token")
                        } else {
                            null
                        }
                        onStatus("Connected")
                    }
                    "hello-deny" -> {
                        val timedOut = json.optString("reason") == "timeout"
                        onStatus(if (timedOut) "Approval timed out" else "Connection denied on PC")
                    }
                    "layout" -> onLayout(json)
                    // 볼륨만 바뀌었을 때 오는 가벼운 메시지 - 아이콘까지 담긴 전체
                    // layout을 다시 안 그리고 볼륨만 갱신한다(드래그 중엔 초당
                    // 여러 번 올 수 있어서, 매번 layout으로 처리하면 느려진다).
                    "volume-sync" -> {
                        val level = json.optDouble("volume", -1.0)
                        if (level >= 0.0) onVolumeSync(level.toFloat())
                    }
                }
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                onStatus("Disconnected")
                scheduleReconnect()
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                onStatus("Disconnected")
                scheduleReconnect()
            }
        })
    }

    private fun scheduleReconnect() {
        if (!wantsConnection) return
        val url = lastUrl ?: return
        val delay = backoffMs.getOrElse(reconnectAttempt) { backoffMs.last() }
        reconnectAttempt++
        mainHandler.postDelayed({
            if (wantsConnection) openSocket(url)
        }, delay)
    }

    fun disconnect() {
        wantsConnection = false
        socket?.cancel()
    }

    fun press(id: String) {
        val message = JSONObject()
        message.put("type", "press")
        message.put("id", id)
        socket?.send(message.toString())
    }

    fun setVolume(level: Float) {
        val message = JSONObject()
        message.put("type", "volume")
        message.put("level", level)
        socket?.send(message.toString())
    }

    companion object {
        private const val KEY_DEVICE_ID = "device_id"
        private const val KEY_PAIRING_TOKEN = "pairing_token"
    }
}
