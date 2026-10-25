package `in`.arena360.hub

import android.Manifest
import android.os.Bundle
import android.view.WindowManager
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts

class MainActivity : TauriActivity() {
  private var afterNotifications: () -> Unit = {}
  private val notifications =
    registerForActivityResult(ActivityResultContracts.RequestPermission()) { afterNotifications() }

  fun askNotifications(done: () -> Unit) {
    afterNotifications = done
    notifications.launch(Manifest.permission.POST_NOTIFICATIONS)
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    super.onCreate(savedInstanceState)
    if (HubService.hasDatabase(this)) HubService.start(this)
  }

  override fun onWebViewCreate(webView: WebView) {
    runOnUiThread { webView.addJavascriptInterface(HubBridge(this), "ArenaHub") }
  }
}
