package `in`.arena360.hub

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import android.webkit.JavascriptInterface
import androidx.core.app.NotificationManagerCompat
import org.json.JSONObject

class HubBridge(private val activity: MainActivity) {
  private val prefs = activity.getSharedPreferences("hub", Context.MODE_PRIVATE)

  @JavascriptInterface
  fun state(): String =
    JSONObject()
      .put("notifications", NotificationManagerCompat.from(activity).areNotificationsEnabled())
      .put("battery_exempt", activity.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(activity.packageName))
      .put("asked", prefs.getBoolean("asked", false))
      .toString()

  @JavascriptInterface
  fun startHub() = HubService.start(activity)

  @JavascriptInterface
  fun stopHub() {
    HubService.stop(activity)
    activity.runOnUiThread { activity.finishAndRemoveTask() }
  }

  @JavascriptInterface
  fun requestAccess() {
    prefs.edit().putBoolean("asked", true).apply()
    activity.runOnUiThread {
      if (Build.VERSION.SDK_INT >= 33 && !NotificationManagerCompat.from(activity).areNotificationsEnabled()) {
        activity.askNotifications { requestBattery() }
      } else {
        requestBattery()
      }
    }
  }

  @JavascriptInterface
  fun skipAccess() = prefs.edit().putBoolean("asked", true).apply()

  @JavascriptInterface
  fun openBatterySettings() {
    activity.runOnUiThread { activity.startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)) }
  }

  @JavascriptInterface
  fun openNotificationSettings() {
    activity.runOnUiThread {
      activity.startActivity(
        Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, activity.packageName),
      )
    }
  }

  private fun requestBattery() {
    if (activity.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(activity.packageName)) return
    activity.startActivity(
      Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:" + activity.packageName)),
    )
  }
}
