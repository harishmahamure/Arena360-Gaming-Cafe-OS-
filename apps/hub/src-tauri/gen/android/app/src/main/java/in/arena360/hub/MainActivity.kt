package `in`.arena360.hub

import android.Manifest
import android.content.res.Configuration
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.IntentSenderRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import com.google.android.play.core.appupdate.AppUpdateInfo
import com.google.android.play.core.appupdate.AppUpdateManager
import com.google.android.play.core.appupdate.AppUpdateOptions
import `in`.arena360.ads.ArenaAds
import kotlin.math.roundToInt
import org.json.JSONObject

class MainActivity : TauriActivity() {
  companion object {
    /** The system font scale is followed up to this share of the normal text size. */
    const val MAX_TEXT_ZOOM = 130

    /** The window event the Hub UI listens for (`apps/hub/src/lib/back.ts`). */
    private const val BACK_SCRIPT = "window.dispatchEvent(new Event('arena:back'))"

    /** Tells the Settings → Printer page the Bluetooth permission prompt was answered. */
    private const val BLUETOOTH_SCRIPT = "window.dispatchEvent(new Event('arena:bluetooth'))"

    /** Tells Settings → System the folder picker closed; `detail.uri` is the chosen tree or null. */
    private fun backupFolderScript(uri: String?) =
      "window.dispatchEvent(new CustomEvent('arena:backup-folder', { detail: { uri: " +
        (if (uri == null) "null" else JSONObject.quote(uri)) + " } }))"

    /** Tells Settings → System the Play update state changed (`none`, `downloading`, `ready`). */
    private fun updateScript(status: String) =
      "window.dispatchEvent(new CustomEvent('arena:update', { detail: { status: " + JSONObject.quote(status) + " } }))"

    /** The text zoom (percent) for a system font scale: it follows the scale, capped at 130%. */
    fun textZoomFor(fontScale: Float): Int = (fontScale * 100).roundToInt().coerceAtMost(MAX_TEXT_ZOOM)
  }

  private var afterNotifications: () -> Unit = {}
  private val notifications =
    registerForActivityResult(ActivityResultContracts.RequestPermission()) { afterNotifications() }

  private var webView: WebView? = null
  private var ads: ArenaAds? = null
  var updates: UpdateBridge? = null
    private set

  // Back goes to the Hub UI, which closes the topmost sheet (or does nothing): it never leaves the
  // app and never walks the WebView history.
  private val back =
    object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        webView?.evaluateJavascript(BACK_SCRIPT, null)
      }
    }

  // The Hub UI handles Back itself (see `back`).
  override val handleBackNavigation: Boolean = false

  // Asked from Settings → Printer on Android 12+; the page waits for the `arena:bluetooth` event.
  private val bluetooth =
    registerForActivityResult(ActivityResultContracts.RequestPermission()) {
      webView?.evaluateJavascript(BLUETOOTH_SCRIPT, null)
    }

  fun askBluetooth() {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
      bluetooth.launch(Manifest.permission.BLUETOOTH_CONNECT)
    } else {
      webView?.evaluateJavascript(BLUETOOTH_SCRIPT, null)
    }
  }

  // Asked from Settings → System; the page waits for the `arena:backup-folder` event. The grant is
  // kept here, before the page sees the folder, so the Hub can write there after a restart.
  private val backupFolder =
    registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { tree ->
      val kept = tree != null && BackupFolderBridge.keep(this, tree)
      webView?.evaluateJavascript(backupFolderScript(if (kept) tree.toString() else null), null)
    }

  fun pickBackupFolder() = backupFolder.launch(null)

  // Play's own prompt for a flexible update; a refusal or failure is asked again at the next check.
  private val updateFlow =
    registerForActivityResult(ActivityResultContracts.StartIntentSenderForResult()) { result ->
      if (result.resultCode != RESULT_OK) updates?.failed()
    }

  fun startUpdateFlow(manager: AppUpdateManager, info: AppUpdateInfo, options: AppUpdateOptions) {
    manager.startUpdateFlowForResult(info, updateFlow, options)
  }

  fun askNotifications(done: () -> Unit) {
    afterNotifications = done
    notifications.launch(Manifest.permission.POST_NOTIFICATIONS)
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    super.onCreate(savedInstanceState)
    applySystemInsets()
    claimBack()
    if (HubService.hasDatabase(this)) HubService.start(this)
  }

  override fun onResume() {
    super.onResume()
    // The last callback added is asked first: take the top again if a plugin added its own.
    claimBack()
    ads?.resume()
    updates?.check()
  }

  override fun onPause() {
    // Background or screen off: the banner stops refreshing and makes no request.
    ads?.pause()
    super.onPause()
  }

  override fun onDestroy() {
    ads?.release()
    ads = null
    updates?.release()
    updates = null
    super.onDestroy()
  }

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    // The system font size changed (the activity is not recreated: `fontScale` is in configChanges).
    webView?.let { applyTextZoom(it) }
  }

  override fun onWebViewCreate(webView: WebView) {
    runOnUiThread {
      this.webView = webView
      applyTextZoom(webView)
      updates?.release()
      updates = UpdateBridge(this) { status -> runOnUiThread { webView.evaluateJavascript(updateScript(status), null) } }
      updates?.check()
      webView.addJavascriptInterface(HubBridge(this), "ArenaHub")
      // Inert until the page's ads gate (Android and Free) calls `init`.
      ads?.release()
      ads = ArenaAds(this, webView).also { webView.addJavascriptInterface(it, "ArenaAds") }
      claimBack()
    }
  }

  private fun claimBack() {
    back.remove()
    onBackPressedDispatcher.addCallback(this, back)
  }

  private fun applyTextZoom(webView: WebView) {
    webView.settings.textZoom = textZoomFor(resources.configuration.fontScale)
  }

  /**
   * Edge-to-edge is enforced from Android 15: the content view gets the status bar, navigation
   * bar (gesture or 3-button), display cutout and keyboard as padding, so no control sits under
   * them and the WebView shrinks above the keyboard.
   */
  private fun applySystemInsets() {
    val content = findViewById<android.view.View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars =
        insets.getInsets(
          WindowInsetsCompat.Type.systemBars() or
            WindowInsetsCompat.Type.displayCutout() or
            WindowInsetsCompat.Type.ime(),
        )
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
    ViewCompat.requestApplyInsets(content)
  }
}
