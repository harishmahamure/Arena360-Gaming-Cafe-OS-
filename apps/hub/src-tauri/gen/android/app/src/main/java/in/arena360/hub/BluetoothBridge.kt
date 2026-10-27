package `in`.arena360.hub

import android.Manifest
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothManager
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import java.io.IOException
import java.util.UUID
import java.util.concurrent.atomic.AtomicReference

/**
 * Bluetooth SPP printing for the Rust printer adapter (`bt_jni.rs` calls these through JNI).
 *
 * It lists printers already paired in Android settings and sends one job over a fresh RFCOMM
 * connection, so the next print after a drop connects again. It never scans or pairs, so the only
 * permission is `BLUETOOTH_CONNECT` (Android 12+); on Android 11 and older the install-time
 * `BLUETOOTH` permission covers it. Every call blocks and runs on a Hub thread, never the UI one.
 */
object BluetoothBridge {
  private const val TAG = "ArenaBluetooth"
  private val SPP = UUID.fromString("00001101-0000-1000-8000-00805F9B34FB")

  /** Lets the printer empty its buffer before the link closes; closing early truncates the page. */
  private const val DRAIN_MS = 600L
  private const val CLOSE_WAIT_MS = 1000L

  const val GRANTED = 0
  const val DENIED = 1
  const val OFF = 2
  const val UNAVAILABLE = 3

  @Volatile private var context: Context? = null

  /** Called by the service before the Hub starts; idempotent. */
  @JvmStatic
  fun attach(app: Context) {
    context = app.applicationContext
  }

  /** Whether the Bluetooth permission is granted (always true before Android 12). */
  @JvmStatic
  fun permitted(app: Context): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
      app.checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED

  private fun adapter(): BluetoothAdapter? =
    (context?.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager)?.adapter

  /** One of [GRANTED], [DENIED], [OFF] or [UNAVAILABLE]. */
  @JvmStatic
  fun access(): Int {
    val app = context ?: return UNAVAILABLE
    if (!permitted(app)) return DENIED
    val adapter = adapter() ?: return UNAVAILABLE
    return if (adapter.isEnabled) GRANTED else OFF
  }

  /** Paired devices as `[address, name, address, name, ...]`; `null` when [access] is not [GRANTED]. */
  @JvmStatic
  fun paired(): Array<String>? {
    if (access() != GRANTED) return null
    val adapter = adapter() ?: return null
    return try {
      adapter.bondedDevices
        .flatMap { listOf(it.address, it.name ?: it.address) }
        .toTypedArray()
    } catch (e: SecurityException) {
      Log.w(TAG, "paired devices are not allowed", e)
      null
    }
  }

  /**
   * Prints [bytes] on the device at [address] and gives up after [timeoutMs]. Returns `null` when
   * the whole page was written, else `permission`, `unavailable` or `link:<plain reason>`.
   */
  @JvmStatic
  fun send(address: String, bytes: ByteArray, timeoutMs: Int): String? {
    when (access()) {
      DENIED -> return "permission"
      OFF, UNAVAILABLE -> return "unavailable"
    }
    val adapter = adapter() ?: return "unavailable"
    val socket =
      try {
        adapter.getRemoteDevice(address).createRfcommSocketToServiceRecord(SPP)
      } catch (e: SecurityException) {
        return "permission"
      } catch (e: IllegalArgumentException) {
        return "link:this is not a Bluetooth address"
      } catch (e: IOException) {
        return "link:could not open a Bluetooth connection"
      }
    val outcome = AtomicReference<String?>(null)
    val worker = Thread({ outcome.set(write(socket, bytes)) }, "arena-bt-print")
    worker.start()
    worker.join(timeoutMs.toLong())
    if (worker.isAlive) {
      // connect() has no timeout of its own: closing the socket is what makes it give up.
      runCatching { socket.close() }
      worker.join(CLOSE_WAIT_MS)
      return "link:the printer did not answer within ${(timeoutMs / 1000).coerceAtLeast(1)} s"
    }
    return outcome.get()
  }

  private fun write(socket: android.bluetooth.BluetoothSocket, bytes: ByteArray): String? {
    try {
      try {
        socket.connect()
      } catch (e: SecurityException) {
        return "permission"
      } catch (e: IOException) {
        Log.w(TAG, "connect failed", e)
        return "link:could not connect. Check that the printer is on, in range and paired"
      }
      try {
        socket.outputStream.write(bytes)
        socket.outputStream.flush()
        Thread.sleep(DRAIN_MS)
      } catch (e: IOException) {
        Log.w(TAG, "write failed", e)
        return "link:the connection dropped while printing"
      } catch (e: InterruptedException) {
        Thread.currentThread().interrupt()
        return "link:printing was interrupted"
      }
      return null
    } finally {
      runCatching { socket.close() }
    }
  }
}
