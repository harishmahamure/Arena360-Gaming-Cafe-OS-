# The Hub reaches the plugin through a WebView JavaScript interface: keep its methods.
-keepclassmembers class in.arena360.ads.ArenaAds {
    @android.webkit.JavascriptInterface <methods>;
}
