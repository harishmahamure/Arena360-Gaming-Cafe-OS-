package `in`.arena360.hub

import com.google.android.play.core.appupdate.AppUpdateManager
import com.google.android.play.core.appupdate.AppUpdateManagerFactory
import com.google.android.play.core.appupdate.AppUpdateOptions
import com.google.android.play.core.install.InstallStateUpdatedListener
import com.google.android.play.core.install.model.AppUpdateType
import com.google.android.play.core.install.model.InstallStatus
import com.google.android.play.core.install.model.UpdateAvailability

/**
 * Play in-app updates (Story 4.16). A flexible update downloads in the background while the Hub keeps
 * serving; once it is downloaded the page offers "Update ready" in System and calls [install] only when
 * no session is active, paused or in grace. If Play installs an update by itself while sessions run, the
 * package replacement restarts the Hub service ([BootReceiver] listens for `MY_PACKAGE_REPLACED`) and
 * kiosks continue offline until it is back.
 */
class UpdateBridge(private val activity: MainActivity, private val emit: (String) -> Unit) {
  companion object {
    /** The Play check is repeated at most this often. */
    const val CHECK_EVERY_MS = 6 * 60 * 60 * 1000L

    /** The status the page sees: `none`, `downloading`, `ready`. */
    fun statusOf(installStatus: Int, availability: Int): String =
      when {
        installStatus == InstallStatus.DOWNLOADED -> "ready"
        installStatus == InstallStatus.DOWNLOADING ||
          installStatus == InstallStatus.PENDING ||
          installStatus == InstallStatus.INSTALLING ||
          availability == UpdateAvailability.DEVELOPER_TRIGGERED_UPDATE_IN_PROGRESS -> "downloading"
        else -> "none"
      }
  }

  private val manager: AppUpdateManager = AppUpdateManagerFactory.create(activity)
  private var lastCheck = 0L

  @Volatile var status = "none"
    private set

  private val listener = InstallStateUpdatedListener { state -> set(statusOf(state.installStatus(), UpdateAvailability.UNKNOWN)) }

  init {
    manager.registerListener(listener)
  }

  private fun set(next: String) {
    if (next == status) return
    status = next
    emit(next)
  }

  /** Asks Play; an available update starts downloading at once (flexible: the Hub is not interrupted). */
  fun check(force: Boolean = false) {
    val now = System.currentTimeMillis()
    if (!force && now - lastCheck < CHECK_EVERY_MS) return
    lastCheck = now
    manager.appUpdateInfo.addOnSuccessListener { info ->
      val next = statusOf(info.installStatus(), info.updateAvailability())
      if (next != "none") return@addOnSuccessListener set(next)
      if (info.updateAvailability() == UpdateAvailability.UPDATE_AVAILABLE && info.isUpdateTypeAllowed(AppUpdateType.FLEXIBLE)) {
        activity.startUpdateFlow(manager, info, AppUpdateOptions.newBuilder(AppUpdateType.FLEXIBLE).build())
        set("downloading")
      }
    }
  }

  /** Installs the downloaded update; the app restarts and the package replacement restarts the service. */
  fun install() {
    if (status == "ready") manager.completeUpdate()
  }

  /** The user refused Play's prompt, or the download failed: ask again at the next check. */
  fun failed() = set("none")

  fun release() = manager.unregisterListener(listener)
}
