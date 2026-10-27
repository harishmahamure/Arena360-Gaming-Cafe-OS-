package `in`.arena360.hub

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.os.Handler
import android.os.Looper
import android.util.Log

/**
 * Advertises the Hub on the LAN through `NsdManager`, with the same service type and TXT
 * records as the Windows Hub, so a kiosk's discovery lists a tablet Hub like any other.
 *
 * The Rust core decides what to advertise (`MdnsTask`) and calls [register] and [unregister]
 * through JNI (see `nsd_jni.rs`). This object keeps `NsdManager` in step with that, and watches
 * the default network: when the address changes it re-registers the service and tells Rust to
 * re-read the addresses ([nativeNetworkChanged]), within a second of the callback.
 */
object NsdBridge {
  private const val TAG = "ArenaNsd"
  private const val SERVICE_TYPE = "_arena360-hub._tcp"
  private const val REREGISTER_DELAY_MS = 500L
  private const val RETRY_DELAY_MS = 5000L
  private const val STOP_TIMEOUT_MS = 5000L

  private class Advert(val instance: String, val port: Int, val txt: Map<String, String>) {
    override fun equals(other: Any?) =
      other is Advert && other.instance == instance && other.port == port && other.txt == txt

    override fun hashCode() = instance.hashCode() * 31 + port
  }

  private val main = Handler(Looper.getMainLooper())
  @Volatile private var context: Context? = null
  private val callbacks = mutableListOf<ConnectivityManager.NetworkCallback>()
  private var wanted: Advert? = null
  private var active: Listener? = null
  private var stopping = false
  private var restart = false

  init {
    System.loadLibrary("arena_hub_ui_lib")
  }

  /** Starts watching the network. Called by the service before the Hub starts; idempotent. */
  @JvmStatic
  @Synchronized
  fun attach(app: Context) {
    if (callbacks.isNotEmpty()) return
    context = app.applicationContext
    val cm = connectivity() ?: return
    try {
      val default = newCallback()
      cm.registerDefaultNetworkCallback(default)
      callbacks.add(default)
      // A cafe Wi-Fi with no internet is not the default network while mobile data is on, so
      // watch the LAN transports as well.
      val lan = newCallback()
      val request = NetworkRequest.Builder()
        .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
        .addTransportType(NetworkCapabilities.TRANSPORT_ETHERNET)
        .build()
      cm.registerNetworkCallback(request, lan)
      callbacks.add(lan)
    } catch (e: RuntimeException) {
      Log.w(TAG, "cannot watch the network", e)
    }
  }

  private fun newCallback() = object : ConnectivityManager.NetworkCallback() {
    override fun onAvailable(network: Network) = changed()

    override fun onLost(network: Network) = changed()

    override fun onLinkPropertiesChanged(network: Network, props: LinkProperties) = changed()
  }

  /** Stops watching and withdraws the service. Called when the service stops. */
  @JvmStatic
  @Synchronized
  fun detach() {
    callbacks.forEach { cb -> runCatching { connectivity()?.unregisterNetworkCallback(cb) } }
    callbacks.clear()
    // Nothing queued may run against the next service instance.
    main.removeCallbacksAndMessages(null)
    wanted = null
    restart = false
    reconcile()
  }

  /** Advertises [instance] on [port] with the given TXT records; replaces any earlier advert. */
  @JvmStatic
  @Synchronized
  fun register(instance: String, port: Int, keys: Array<String>, values: Array<String>): Boolean {
    if (manager() == null) return false
    wanted = Advert(instance, port, keys.zip(values).toMap())
    reconcile()
    return true
  }

  @JvmStatic
  @Synchronized
  fun unregister() {
    wanted = null
    reconcile()
  }

  /** What the Hub is connected to: "Wi-Fi", "Wi-Fi: <name>" when Android reveals it, "Ethernet". */
  @JvmStatic
  fun networkName(): String? {
    val cm = connectivity() ?: return null
    val caps = lanCapabilities(cm) ?: return null
    return when {
      caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "Ethernet"
      caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> wifiName()?.let { "Wi-Fi: $it" } ?: "Wi-Fi"
      else -> null
    }
  }

  /** The active network when it is a LAN transport, else any connected Wi-Fi or Ethernet one. */
  @Suppress("DEPRECATION")
  private fun lanCapabilities(cm: ConnectivityManager): NetworkCapabilities? {
    fun isLan(c: NetworkCapabilities?) = c != null &&
      (c.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) ||
        c.hasTransport(NetworkCapabilities.TRANSPORT_WIFI))
    val active = cm.activeNetwork?.let { cm.getNetworkCapabilities(it) }
    if (isLan(active)) return active
    return runCatching { cm.allNetworks.map { cm.getNetworkCapabilities(it) }.firstOrNull(::isLan) }
      .getOrNull()
  }

  @Suppress("DEPRECATION")
  private fun wifiName(): String? {
    val wm = context?.getSystemService(Context.WIFI_SERVICE) as? WifiManager ?: return null
    val ssid = runCatching { wm.connectionInfo?.ssid }.getOrNull()?.trim('"') ?: return null
    // Without a location permission Android answers with a placeholder.
    return ssid.takeIf { it.isNotBlank() && it != WifiManager.UNKNOWN_SSID }
  }

  private fun changed() {
    // Let the new address settle, then re-register on it and tell Rust to re-read.
    main.removeCallbacks(onChanged)
    main.postDelayed(onChanged, REREGISTER_DELAY_MS)
  }

  private val onChanged = Runnable {
    synchronized(this) {
      restart = true
      reconcile()
    }
    nativeNetworkChanged()
  }

  private fun connectivity() = context?.getSystemService(ConnectivityManager::class.java)

  private fun manager() = context?.getSystemService(Context.NSD_SERVICE) as? NsdManager

  /**
   * Brings `NsdManager` in line with [wanted]: stops an advert that is no longer wanted (or must
   * restart on the new address), and starts the wanted one once nothing is registered. A stop
   * finishes asynchronously, so the start waits for [Listener.onServiceUnregistered].
   */
  private fun reconcile() {
    val nsd = manager() ?: return
    if (stopping) return
    val want = wanted
    val current = active
    if (current != null && (want == null || current.advert != want || restart)) {
      stopping = true
      restart = false
      // If the stop is never reported, do not stay blocked for good.
      main.postDelayed({
        synchronized(this) {
          if (stopping && active === current) {
            stopping = false
            active = null
            reconcile()
          }
        }
      }, STOP_TIMEOUT_MS)
      try {
        nsd.unregisterService(current)
      } catch (e: IllegalArgumentException) {
        // Never finished registering: nothing to wait for.
        stopping = false
        active = null
        reconcile()
      }
      return
    }
    restart = false
    if (current == null && want != null) {
      val info = NsdServiceInfo().apply {
        serviceName = want.instance
        serviceType = SERVICE_TYPE
        port = want.port
        want.txt.forEach { (k, v) -> setAttribute(k, v) }
      }
      val listener = Listener(want)
      active = listener
      try {
        nsd.registerService(info, NsdManager.PROTOCOL_DNS_SD, listener)
      } catch (e: RuntimeException) {
        Log.w(TAG, "register failed", e)
        active = null
        retryLater()
      }
    }
  }

  private val retry = Runnable { synchronized(this) { reconcile() } }

  private fun retryLater() {
    main.removeCallbacks(retry)
    main.postDelayed(retry, RETRY_DELAY_MS)
  }

  private class Listener(val advert: Advert) : NsdManager.RegistrationListener {
    override fun onServiceRegistered(info: NsdServiceInfo) {
      Log.i(TAG, "advertised ${info.serviceName}")
    }

    override fun onRegistrationFailed(info: NsdServiceInfo, errorCode: Int) {
      Log.w(TAG, "registration failed: $errorCode")
      synchronized(NsdBridge) {
        if (active === this) {
          active = null
          retryLater()
        }
      }
    }

    override fun onServiceUnregistered(info: NsdServiceInfo) = stopped()

    override fun onUnregistrationFailed(info: NsdServiceInfo, errorCode: Int) = stopped()

    private fun stopped() {
      synchronized(NsdBridge) {
        if (active === this) active = null
        stopping = false
        reconcile()
      }
    }
  }

  /** Tells the Rust core the network changed, so it re-reads the addresses now. */
  @JvmStatic external fun nativeNetworkChanged()
}
