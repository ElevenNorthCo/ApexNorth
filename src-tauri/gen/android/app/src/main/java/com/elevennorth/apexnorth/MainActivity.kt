package com.elevennorth.apexnorth

import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : TauriActivity() {
  /** Space taken by the camera cutout and any visible system bars, in CSS px. */
  @Volatile private var insets = "{\"top\":0,\"right\":0,\"bottom\":0,\"left\":0}"

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // A race dashboard wants the whole screen: hide the status and
    // navigation bars; a swipe from the edge shows them for a moment.
    val controller = WindowCompat.getInsetsController(window, window.decorView)
    controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
    controller.hide(WindowInsetsCompat.Type.systemBars())
  }

  override fun onWebViewCreate(webView: WebView) {
    // The page reads these as --an-inset-* (Android's WebView doesn't fill
    // in CSS env(safe-area-inset-*) for an edge-to-edge app).
    webView.addJavascriptInterface(Bridge(), "ApexNorthAndroid")
    ViewCompat.setOnApplyWindowInsetsListener(webView) { view, all ->
      val i = all.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val d = resources.displayMetrics.density
      insets = "{\"top\":${i.top / d},\"right\":${i.right / d},\"bottom\":${i.bottom / d},\"left\":${i.left / d}}"
      (view as WebView).evaluateJavascript("window.dispatchEvent(new Event('an-insets'))", null)
      all
    }
  }

  inner class Bridge {
    @JavascriptInterface
    fun insets(): String = this@MainActivity.insets
  }
}
