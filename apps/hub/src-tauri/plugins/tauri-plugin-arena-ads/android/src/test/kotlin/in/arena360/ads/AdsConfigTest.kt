package `in`.arena360.ads

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AdsConfigTest {
  @Test
  fun `the rating defaults to PG and MA is never accepted`() {
    assertEquals(AdRating.PG, AdsConfig.parse(null).rating)
    assertEquals(AdRating.PG, AdsConfig.parse("{}").rating)
    assertEquals(AdRating.PG, AdsConfig.parse("""{"rating":"MA"}""").rating)
    assertEquals(AdRating.PG, AdsConfig.parse("not json").rating)
    assertEquals(AdRating.G, AdsConfig.parse("""{"rating":"G"}""").rating)
    assertEquals(AdRating.T, AdsConfig.parse("""{"rating":"T"}""").rating)
  }

  @Test
  fun `requests are non-personalised until the consent is recorded`() {
    assertEquals(false, AdsConfig.parse(null).personalised)
    assertEquals(false, AdsConfig.parse("{}").personalised)
    assertEquals(false, AdsConfig.parse("""{"rating":"G"}""").personalised)
    assertEquals(false, AdsConfig.parse("not json").personalised)
    assertEquals(false, AdsConfig.parse("""{"personalised":"yes"}""").personalised)
    assertEquals(true, AdsConfig.parse("""{"rating":"T","personalised":true}""").personalised)
    assertEquals(AdsConfig(AdRating.T, true), AdsConfig.parse("""{"rating":"T","personalised":true}"""))
  }

  @Test
  fun `a banner rect needs four finite numbers and a size`() {
    assertEquals(BannerRect(0.0, 800.0, 412.0, 50.0), BannerRect.parse("""{"x":0,"y":800,"width":412,"height":50}"""))
    assertNull(BannerRect.parse(null))
    assertNull(BannerRect.parse("""{"x":0,"y":800,"width":0,"height":50}"""))
    assertNull(BannerRect.parse("""{"x":0,"y":800}"""))
    assertNull(BannerRect.parse("nope"))
  }

  @Test
  fun `an event script names the event and the slot`() {
    val s = AdEvents.script(AdEvents.SHOWN, AdEvents.BANNER)
    assertEquals(true, s.contains("\"ad.shown\"") && s.contains("\"banner\"") && s.contains("arena:ad"))
  }
}
