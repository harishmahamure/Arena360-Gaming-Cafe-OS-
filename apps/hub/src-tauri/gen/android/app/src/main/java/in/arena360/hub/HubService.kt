package `in`.arena360.hub

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import org.json.JSONObject

class HubService : Service() {
  private var wake: PowerManager.WakeLock? = null
  private val wifi = mutableListOf<WifiManager.WifiLock>()
  @Volatile private var poller: Thread? = null

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onCreate() {
    super.onCreate()
    val channel = NotificationChannel(CHANNEL, getString(R.string.hub_channel), NotificationManager.IMPORTANCE_LOW)
    getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == ACTION_STOP) {
      Thread { stopHub() }.start()
      return START_NOT_STICKY
    }
    ServiceCompat.startForeground(this, NOTIFICATION_ID, notification(null, 0), ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
    acquireLocks()
    Thread { begin() }.start()
    return START_STICKY
  }

  private fun begin() {
    if (!nativeStart(dataDir(this).absolutePath)) {
      stopForeground(STOP_FOREGROUND_REMOVE)
      stopSelf()
      return
    }
    if (poller != null) return
    poller = Thread { poll() }.also { it.start() }
  }

  private fun poll() {
    var shown: Pair<String?, Int>? = null
    while (!Thread.currentThread().isInterrupted) {
      val now = status()
      if (now != null && now != shown) {
        shown = now
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification(now.first, now.second))
      }
      try {
        Thread.sleep(POLL_MS)
      } catch (e: InterruptedException) {
        return
      }
    }
  }

  private fun status(): Pair<String?, Int>? {
    val conn = URL("http://127.0.0.1:7409/ui/snapshot").openConnection() as HttpURLConnection
    return try {
      conn.connectTimeout = 2000
      conn.readTimeout = 4000
      val body = JSONObject(conn.inputStream.bufferedReader().use { it.readText() })
      val devices = body.optJSONArray("devices")
      var online = 0
      for (i in 0 until (devices?.length() ?: 0)) {
        if (devices!!.getJSONObject(i).optBoolean("online")) online++
      }
      Pair(body.optJSONObject("branch")?.optString("name")?.takeIf { it.isNotEmpty() }, online)
    } catch (e: Exception) {
      null
    } finally {
      conn.disconnect()
    }
  }

  private fun notification(branch: String?, online: Int): Notification {
    val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
    return NotificationCompat.Builder(this, CHANNEL)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setContentTitle(branch ?: getString(R.string.app_name))
      .setContentText(resources.getQuantityString(R.plurals.hub_running, online, online))
      .setContentIntent(open)
      .setOngoing(true)
      .setOnlyAlertOnce(true)
      .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
      .build()
  }

  private fun acquireLocks() {
    if (wake == null) {
      wake = getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "arena:hub").apply {
        setReferenceCounted(false)
        acquire()
      }
    }
    if (wifi.isEmpty()) {
      val wm = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
      val modes = mutableListOf(@Suppress("DEPRECATION") WifiManager.WIFI_MODE_FULL_HIGH_PERF)
      if (Build.VERSION.SDK_INT >= 29) modes.add(WifiManager.WIFI_MODE_FULL_LOW_LATENCY)
      for (mode in modes) {
        wifi.add(wm.createWifiLock(mode, "arena:hub").apply {
          setReferenceCounted(false)
          acquire()
        })
      }
    }
  }

  private fun releaseLocks() {
    wifi.forEach { if (it.isHeld) it.release() }
    wifi.clear()
    wake?.let { if (it.isHeld) it.release() }
    wake = null
  }

  private fun stopHub() {
    poller?.interrupt()
    poller = null
    nativeStop()
    releaseLocks()
    stopForeground(STOP_FOREGROUND_REMOVE)
    stopSelf()
  }

  override fun onDestroy() {
    poller?.interrupt()
    releaseLocks()
    super.onDestroy()
  }

  companion object {
    private const val CHANNEL = "hub_service"
    private const val NOTIFICATION_ID = 1
    private const val POLL_MS = 5000L
    const val ACTION_STOP = "in.arena360.hub.STOP"

    init {
      System.loadLibrary("arena_hub_ui_lib")
    }

    @JvmStatic external fun nativeStart(dir: String): Boolean

    @JvmStatic external fun nativeStop()

    fun dataDir(context: Context): File = File(context.dataDir, "in.arena360.hub/files")

    fun hasDatabase(context: Context): Boolean = File(dataDir(context), "branch.db").exists()

    fun start(context: Context) {
      ContextCompat.startForegroundService(context, Intent(context, HubService::class.java))
    }

    fun stop(context: Context) {
      context.startService(Intent(context, HubService::class.java).setAction(ACTION_STOP))
    }
  }
}
