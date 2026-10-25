package `in`.arena360.hub

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class BootReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    if (HubService.hasDatabase(context)) HubService.start(context)
  }
}
