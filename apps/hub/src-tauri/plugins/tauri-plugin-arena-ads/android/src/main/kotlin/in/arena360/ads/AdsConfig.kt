package `in`.arena360.ads

import org.json.JSONObject

/** Content ratings the Hub may ask for. MA is never offered, so it is not a case here. */
enum class AdRating(val wire: String, val sdk: String) {
  G("G", "G"),
  PG("PG", "PG"),
  T("T", "T");

  companion object {
    /** The default is PG; anything unknown (including "MA") falls back to it, never to a looser rating. */
    fun parse(value: String?): AdRating = values().firstOrNull { it.wire == value } ?: PG
  }
}

/** What `init` receives from the Hub UI: `{"rating":"PG"}`. */
data class AdsConfig(val rating: AdRating) {
  companion object {
    fun parse(json: String?): AdsConfig =
      try {
        AdsConfig(AdRating.parse(JSONObject(json ?: "{}").optString("rating", null)))
      } catch (_: Exception) {
        AdsConfig(AdRating.PG)
      }
  }
}

/** A banner rectangle in CSS pixels, relative to the WebView's viewport. */
data class BannerRect(val x: Double, val y: Double, val width: Double, val height: Double) {
  companion object {
    private const val MAX_SIDE = 10_000.0

    /** Null for anything that is not four finite numbers with a size of 1 to 10000 CSS pixels. */
    fun parse(json: String?): BannerRect? =
      try {
        val o = JSONObject(json ?: return null)
        val r = BannerRect(o.getDouble("x"), o.getDouble("y"), o.getDouble("width"), o.getDouble("height"))
        r.takeIf {
          listOf(it.x, it.y, it.width, it.height).all { v -> v.isFinite() } && it.width >= 1 && it.height >= 1 &&
            it.width <= MAX_SIDE && it.height <= MAX_SIDE
        }
      } catch (_: Exception) {
        null
      }
  }
}

/** The events the plugin sends to the page, as `arena:ad` window events. */
object AdEvents {
  const val REQUESTED = "ad.requested"
  const val LOADED = "ad.loaded"
  const val FAILED = "ad.failed"
  const val SHOWN = "ad.shown"
  const val CLICKED = "ad.clicked"
  const val DISMISSED = "ad.dismissed"

  const val BANNER = "banner"
  const val INTERSTITIAL = "interstitial"

  /** The script that delivers one event; `slot` is `banner` or `interstitial`. */
  fun script(event: String, slot: String): String =
    "window.dispatchEvent(new CustomEvent('arena:ad', { detail: { event: " +
      JSONObject.quote(event) + ", slot: " + JSONObject.quote(slot) + " } }))"
}
