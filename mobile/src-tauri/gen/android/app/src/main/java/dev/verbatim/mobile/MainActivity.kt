package dev.verbatim.mobile

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.view.WindowManager
import android.view.inputmethod.InputMethodManager
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // Sokki is always dark: light status and navigation bar icons.
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)
    askForLocalNetwork()
    fromKeyboard(intent)
    pairingLink(intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    fromKeyboard(intent)
    pairingLink(intent)
  }

  private var webView: WebView? = null
  /** A pairing ticket from a link, until the page picks it up. */
  @Volatile private var pendingTicket: String? = null

  /** https://sokki.org/p#sokki1:… or sokki://pair#sokki1:… : the ticket is
   * the part after #, which never reaches a server. */
  private fun pairingLink(intent: Intent?) {
    if (intent?.action != Intent.ACTION_VIEW) return
    val ticket = intent.data?.fragment?.takeIf { it.startsWith("sokki1:") } ?: return
    pendingTicket = ticket
    // Page already loaded: hand it over now (else it asks on start).
    webView?.post { webView?.evaluateJavascript("window.__sokkiPairLink && window.__sokkiPairLink()", null) }
  }

  /** How often the keyboard finished a dictation (for the "get Sokki on your computer" card).
   * The keyboard runs in its own process and writes this file. */
  private fun keyboardDictations(): Int =
    java.io.File(filesDir, "keyboard_dictations").takeIf { it.exists() }?.readText()?.trim()?.toIntOrNull() ?: 0

  /** The keyboard's mic sends people here when the microphone isn't allowed yet. */
  private fun fromKeyboard(intent: Intent?) {
    if (intent?.getBooleanExtra("sokki.from_keyboard", false) != true) return
    if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
      requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 2)
    }
  }

  /** Android 17 blocks connections to the local network (the Mac) without this. */
  private fun askForLocalNetwork() {
    if (Build.VERSION.SDK_INT < 37) return
    val permission = "android.permission.ACCESS_LOCAL_NETWORK"
    if (checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED) {
      requestPermissions(arrayOf(permission), 1)
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    webView.addJavascriptInterface(KeyboardSetup(), "SokkiKeyboard")
    webView.addJavascriptInterface(Links(), "SokkiLinks")
    webView.addJavascriptInterface(Screen(), "SokkiScreen")
  }

  /** `window.SokkiScreen`: the screen stays on while a meeting records. With
   * the screen off the app goes to the background, and since Android 9 a
   * background app gets only silence from the microphone: the meeting would
   * be cut off, as on the Mac that went to sleep on 2026-11-19. The power
   * button still turns the screen off; only a foreground service would
   * survive that. */
  inner class Screen {
    @JavascriptInterface
    fun keepOn(on: Boolean) = runOnUiThread {
      if (on) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      else window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }
  }

  /** Links in and out: pairing links, and sharing Sokki's download link. */
  inner class Links {
    /** The ticket from a pairing link, once. */
    @JavascriptInterface
    fun takeTicket(): String? = pendingTicket.also { pendingTicket = null }

    /** Android's share sheet with a text (e.g. sokki.org/get?s=android). */
    @JavascriptInterface
    fun share(text: String) = runOnUiThread {
      val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
      startActivity(Intent.createChooser(send, null))
    }

    @JavascriptInterface
    fun keyboardDictations(): Int = this@MainActivity.keyboardDictations()
  }

  /** The page's keyboard setup: `window.SokkiKeyboard` (see mobile/src/KeyboardSetup.tsx). */
  inner class KeyboardSetup {
    private val imm get() = getSystemService(InputMethodManager::class.java)

    /** "off" (not turned on), "on" (turned on, not chosen) or "active". */
    @JavascriptInterface
    fun state(): String {
      val enabled = imm.enabledInputMethodList.any { it.packageName == packageName }
      if (!enabled) return "off"
      val current = Settings.Secure.getString(contentResolver, Settings.Secure.DEFAULT_INPUT_METHOD) ?: ""
      return if (current.startsWith("$packageName/")) "active" else "on"
    }

    @JavascriptInterface
    fun micAllowed(): Boolean =
      checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

    /** Step 1: Android's list of keyboards, where Sokki is switched on. */
    @JavascriptInterface
    fun turnOn() = runOnUiThread {
      startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    /** Step 2: Android's keyboard picker. */
    @JavascriptInterface
    fun choose() = runOnUiThread { imm.showInputMethodPicker() }

    @JavascriptInterface
    fun allowMic() = runOnUiThread {
      requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 2)
    }

    /** The keyboard's own settings (layouts, languages, look). */
    @JavascriptInterface
    fun openSettings() = runOnUiThread {
      startActivity(Intent().setClassName(packageName, "helium314.keyboard.settings.SettingsActivity"))
    }
  }
}
