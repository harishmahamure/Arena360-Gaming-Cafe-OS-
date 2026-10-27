package `in`.arena360.ads

import android.app.Activity
import android.os.StrictMode
import android.os.Bundle
import android.util.Log
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.widget.FrameLayout
import com.google.ads.mediation.admob.AdMobAdapter
import com.google.android.gms.ads.AdError
import com.google.android.gms.ads.AdListener
import com.google.android.gms.ads.AdRequest
import com.google.android.gms.ads.AdSize
import com.google.android.gms.ads.AdView
import com.google.android.gms.ads.FullScreenContentCallback
import com.google.android.gms.ads.LoadAdError
import com.google.android.gms.ads.MobileAds
import com.google.android.gms.ads.RequestConfiguration
import com.google.android.gms.ads.interstitial.InterstitialAd
import com.google.android.gms.ads.interstitial.InterstitialAdLoadCallback
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import org.json.JSONObject

/**
 * The only code in the repo that imports `com.google.android.gms.ads`. The Hub page reaches it
 * through the `ArenaAds` JavaScript interface, and only when the ads gate is on (Android, Free).
 * Until the page calls [init] nothing here touches the SDK, so a paid Hub never makes an ad request.
 *
 * The surface is `init`, `bannerPlace`, `bannerHide`, `bannerDestroy`, `interstitialPreload` and
 * `interstitialShow` (the page's `ads.ts` names them `banner.place` and so on). Events go back as
 * `arena:ad` window events. The SDK is started on a background thread; `loadAd` and `show` are
 * the SDK's own main-thread calls (it does its network and disk work on its own threads), and in
 * a debug build StrictMode logs any network or disk access on the main thread that starts in here.
 */
class ArenaAds(private val activity: Activity, private val webView: WebView) {
  private val worker =
    Executors.newSingleThreadExecutor { r -> Thread(r, "arena-ads").apply { isDaemon = true } }
  private val started = AtomicBoolean(false)
  @Volatile private var ready = false
  @Volatile private var config = AdsConfig(AdRating.PG)

  private var banner: AdView? = null
  private var bannerRequested = false
  private var bannerWidth = 0
  /** The activity is paused (in the background or the screen is off): no banner request is made. */
  private var paused = false
  private var interstitial: InterstitialAd? = null
  private var loadingInterstitial = false
  /** Bumped when the config changes, so a load that began under the old one is discarded. */
  private var interstitialGeneration = 0

  /**
   * Starts the SDK (once) off the main thread. Returns `{"debug":true|false}` at once: the dev
   * gallery only opens on a debug build. `config` is `{"rating":"G"|"PG"|"T","personalised":bool}`.
   */
  @JavascriptInterface
  fun init(config: String?): String {
    this.config = AdsConfig.parse(config)
    if (started.compareAndSet(false, true)) {
      // A thread policy covers only the thread that sets it, and `init` runs on the WebView's bridge thread.
      if (BuildConfig.DEBUG) activity.runOnUiThread { AdsStrictMode.install(activity) }
      worker.execute {
        MobileAds.setRequestConfiguration(requestConfiguration())
        MobileAds.initialize(activity.applicationContext) {}
        ready = true
      }
    } else {
      // Queued behind the start-up task on the single worker, so a later rating is never dropped.
      worker.execute { MobileAds.setRequestConfiguration(requestConfiguration()) }
    }
    return JSONObject().put("debug", BuildConfig.DEBUG).toString()
  }

  private fun requestConfiguration(): RequestConfiguration =
    MobileAds.getRequestConfiguration().toBuilder().setMaxAdContentRating(config.rating.sdk).build()

  /**
   * The branch's rating or consent changed (`{"rating":"G"|"PG"|"T","personalised":bool}`). The
   * rating is the SDK's request configuration, so every request after this one (banner refreshes
   * and interstitials too) carries it, with no restart. A changed rating or consent re-requests
   * the banner (a refresh would reuse the request it was first loaded with) and drops a preloaded
   * interstitial, so nothing loaded under the old rating or consent is shown after the change.
   */
  @JavascriptInterface
  fun configure(config: String?) {
    val next = AdsConfig.parse(config)
    val changed = next != this.config
    this.config = next
    if (!started.get()) return
    worker.execute {
      MobileAds.setRequestConfiguration(requestConfiguration())
      // After the SDK has the new rating, so the re-request cannot go out with the old one.
      if (changed) activity.runOnUiThread { reloadForConfig() }
    }
  }

  private fun reloadForConfig() {
    interstitialGeneration++
    interstitial = null
    loadingInterstitial = false
    val view = banner ?: return
    bannerRequested = false
    if (view.visibility == View.VISIBLE) requestBanner(view)
  }

  /** Every banner and interstitial request: non-personalised until the owner's consent is recorded. */
  private fun adRequest(): AdRequest {
    val builder = AdRequest.Builder()
    if (!config.personalised) {
      builder.addNetworkExtrasBundle(AdMobAdapter::class.java, Bundle().apply { putString("npa", "1") })
    }
    return builder.build()
  }

  /** Puts an anchored adaptive banner inside `rect` (`{x,y,width,height}` in CSS pixels of the page). */
  @JavascriptInterface
  fun bannerPlace(rect: String?) {
    val r = BannerRect.parse(rect) ?: return
    if (!started.get()) return
    activity.runOnUiThread {
      val host = webView.parent as? ViewGroup ?: return@runOnUiThread
      val density = activity.resources.displayMetrics.density
      val view = banner ?: newBanner().also { banner = it; if (paused) it.pause() }
      val params =
        FrameLayout.LayoutParams(
            (r.width * density).toInt(),
            (r.height * density).toInt(),
            Gravity.TOP or Gravity.START,
          )
          .apply {
            leftMargin = webView.left + (r.x * density).toInt()
            topMargin = webView.top + (r.y * density).toInt()
          }
      if (view.parent == null) host.addView(view, params) else view.layoutParams = params
      view.visibility = View.VISIBLE
      val width = r.width.toInt()
      if (width != bannerWidth) {
        // A new width (rotation, resize) gets a new adaptive size, and so a new request.
        bannerWidth = width
        bannerRequested = false
      }
      requestBanner(view)
    }
  }

  /** Requests the banner once per width, and never while the activity is paused (main thread). */
  private fun requestBanner(view: AdView) {
    if (paused || bannerRequested || bannerWidth <= 0) return
    bannerRequested = true
    view.setAdSize(
      AdSize.getCurrentOrientationAnchoredAdaptiveBannerAdSize(activity, bannerWidth)
    )
    emit(AdEvents.REQUESTED, AdEvents.BANNER)
    view.loadAd(adRequest())
  }

  /**
   * Called from the activity's `onPause` (background or screen off): the banner stops refreshing
   * and a placement made meanwhile waits for [resume] before it requests anything.
   */
  fun pause() {
    paused = true
    banner?.pause()
  }

  /** Called from the activity's `onResume`: the banner refreshes again; a waiting placement is requested. */
  fun resume() {
    paused = false
    banner?.let {
      it.resume()
      // A hidden banner (its strip is gone) waits for the next placement instead.
      if (it.visibility == View.VISIBLE) requestBanner(it)
    }
  }

  @JavascriptInterface
  fun bannerHide() {
    activity.runOnUiThread { banner?.visibility = View.GONE }
  }

  @JavascriptInterface
  fun bannerDestroy() {
    activity.runOnUiThread {
      banner?.let {
        (it.parent as? ViewGroup)?.removeView(it)
        it.destroy()
      }
      banner = null
      bannerRequested = false
      bannerWidth = 0
    }
  }

  /** Loads an interstitial ahead of time so [interstitialShow] can show it at once. */
  @JavascriptInterface
  fun interstitialPreload() {
    if (!started.get()) return
    activity.runOnUiThread {
      if (interstitial != null || loadingInterstitial) return@runOnUiThread
      loadingInterstitial = true
      val generation = interstitialGeneration
      emit(AdEvents.REQUESTED, AdEvents.INTERSTITIAL)
      InterstitialAd.load(
        activity,
        BuildConfig.INTERSTITIAL_UNIT_ID,
        adRequest(),
        object : InterstitialAdLoadCallback() {
          override fun onAdLoaded(ad: InterstitialAd) {
            if (generation != interstitialGeneration) return
            loadingInterstitial = false
            interstitial = ad
            emit(AdEvents.LOADED, AdEvents.INTERSTITIAL)
          }

          override fun onAdFailedToLoad(error: LoadAdError) {
            if (generation != interstitialGeneration) return
            loadingInterstitial = false
            interstitial = null
            emit(AdEvents.FAILED, AdEvents.INTERSTITIAL)
          }
        },
      )
    }
  }

  /** Shows the preloaded interstitial, if there is one; Google controls its close timing. */
  @JavascriptInterface
  fun interstitialShow() {
    activity.runOnUiThread {
      val ad =
        interstitial
          ?: run {
            // Tell the page nothing is showing, so it does not wait for a dismiss.
            emit(AdEvents.FAILED, AdEvents.INTERSTITIAL)
            return@runOnUiThread
          }
      interstitial = null
      ad.fullScreenContentCallback =
        object : FullScreenContentCallback() {
          override fun onAdShowedFullScreenContent() = emit(AdEvents.SHOWN, AdEvents.INTERSTITIAL)

          override fun onAdClicked() = emit(AdEvents.CLICKED, AdEvents.INTERSTITIAL)

          override fun onAdDismissedFullScreenContent() = emit(AdEvents.DISMISSED, AdEvents.INTERSTITIAL)

          override fun onAdFailedToShowFullScreenContent(error: AdError) =
            emit(AdEvents.FAILED, AdEvents.INTERSTITIAL)
        }
      ad.show(activity)
    }
  }

  /** Called from the activity's `onDestroy`. */
  fun release() {
    bannerDestroy()
    interstitial = null
    worker.shutdown()
  }

  private fun newBanner(): AdView =
    AdView(activity).apply {
      adUnitId = BuildConfig.BANNER_UNIT_ID
      adListener =
        object : AdListener() {
          override fun onAdLoaded() = emit(AdEvents.LOADED, AdEvents.BANNER)

          override fun onAdFailedToLoad(error: LoadAdError) = emit(AdEvents.FAILED, AdEvents.BANNER)

          override fun onAdImpression() = emit(AdEvents.SHOWN, AdEvents.BANNER)

          override fun onAdClicked() = emit(AdEvents.CLICKED, AdEvents.BANNER)

          override fun onAdClosed() = emit(AdEvents.DISMISSED, AdEvents.BANNER)
        }
    }

  private fun emit(event: String, slot: String) {
    webView.post { webView.evaluateJavascript(AdEvents.script(event, slot), null) }
  }
}

/**
 * In a debug build, logs (as an error) any main-thread network or disk access whose stack passes
 * through this plugin, so a regression shows up in `adb logcat -s ArenaAds`. Violations from other
 * code are not the plugin's and are left alone.
 */
internal object AdsStrictMode {
  private const val TAG = "ArenaAds"

  fun install(activity: Activity) {
    val policy =
      StrictMode.ThreadPolicy.Builder()
        .detectNetwork()
        .detectDiskReads()
        .detectDiskWrites()
        .penaltyListener(activity.mainExecutor) { v ->
          if (v.stackTrace.any { it.className.startsWith("in.arena360.ads.") }) {
            Log.e(TAG, "main-thread access from the ads plugin", v)
          }
        }
        .build()
    StrictMode.setThreadPolicy(policy)
  }
}
