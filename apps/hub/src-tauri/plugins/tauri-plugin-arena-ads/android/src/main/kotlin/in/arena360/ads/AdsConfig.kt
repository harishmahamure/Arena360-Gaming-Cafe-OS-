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

/**
 * What `init` and `configure` receive from the Hub UI: `{"rating":"PG","personalised":false}`.
 * `personalised` is true only once the owner's ads consent is recorded; until then every request
 * is non-personalised. Anything missing or unreadable is the strictest default: PG, non-personalised.
 */
data class AdsConfig(val rating: AdRating, val personalised: Boolean = false) {
  companion object {
    fun parse(json: String?): AdsConfig =
      try {
        val o = JSONObject(json ?: "{}")
        AdsConfig(AdRating.parse(o.optString("rating", null)), o.optBoolean("personalised", false))
      } catch (_: Exception) {
        AdsConfig(AdRating.PG, false)
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

  /**
   * The script that delivers one event; `slot` is `banner` or `interstitial`. `id` is the event's
   * own ULID: the Hub uses it to count the event once, however often it is delivered. It names
   * the event only, never a device, creative or advertiser.
   */
  fun script(event: String, slot: String, id: String = EventIds.next()): String =
    "window.dispatchEvent(new CustomEvent('arena:ad', { detail: { event: " +
      JSONObject.quote(event) + ", slot: " + JSONObject.quote(slot) + ", id: " + JSONObject.quote(id) +
      " } }))"
}

/** ULIDs for events: 48 bits of time then 80 random bits, in Crockford base32 (26 characters). */
object EventIds {
  private const val ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"
  private val random = java.security.SecureRandom()

  fun next(now: Long = System.currentTimeMillis()): String {
    val out = CharArray(26)
    var t = now
    for (i in 9 downTo 0) {
      out[i] = ALPHABET[(t % 32).toInt()]
      t /= 32
    }
    val bytes = ByteArray(16).also { random.nextBytes(it) }
    for (i in 0 until 16) out[10 + i] = ALPHABET[(bytes[i].toInt() and 0xff) % 32]
    return String(out)
  }
}
