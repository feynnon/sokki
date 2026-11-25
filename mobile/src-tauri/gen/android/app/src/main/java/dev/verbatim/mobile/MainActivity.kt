package dev.verbatim.mobile

import android.Manifest
import android.app.StatusBarManager
import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.drawable.Icon
import android.media.AudioManager
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.util.Log
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
    // Not when Android recreates Sokki or it's reopened from recents: that
    // would start a recording nobody asked for.
    val fromHistory = (intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) != 0
    if (savedInstanceState == null && !fromHistory) fromTile(intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    fromKeyboard(intent)
    pairingLink(intent)
    fromTile(intent)
  }

  override fun onDestroy() {
    if (Recording.page?.get() == webView) {
      Recording.page = null
      Recording.on = false
    }
    super.onDestroy()
  }

  companion object {
    /** The Quick Settings tile opens Sokki with this to start recording. */
    const val ACTION_RECORD = "dev.verbatim.mobile.RECORD"
  }

  /** Set when the tile opened Sokki to record, until the page starts. */
  @Volatile private var pendingRecord = false

  private fun fromTile(intent: Intent?) {
    if (intent?.action != ACTION_RECORD) return
    pendingRecord = true
    // Page already loaded: start now (else it asks on start).
    webView?.post { webView?.evaluateJavascript("window.__sokkiRecord && window.__sokkiRecord()", null) }
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
    webView.addJavascriptInterface(Recorder(), "SokkiRecorder")
    webView.addJavascriptInterface(JournalBridge(this), "SokkiJournal")
    Recording.page = java.lang.ref.WeakReference(webView)
  }

  /** `window.SokkiRecorder`: the Quick Settings tile (RecordTile.kt), and
   * whether Android silences the recording. */
  inner class Recorder {
    /** True once after the tile opened Sokki to record. */
    @JavascriptInterface
    fun takeStart(): Boolean = pendingRecord.also { pendingRecord = false }

    /** The page reports when it starts and stops recording, for the tile. */
    @JavascriptInterface
    fun recording(on: Boolean) {
      Recording.on = on
    }

    /** Android's own "Add tile" dialog (Android 13+), once per install. No
     * app can put a tile into the panel by itself, nor choose the wide
     * tile with text: the user widens it in the panel's edit mode. */
    @JavascriptInterface
    fun offerTile() = runOnUiThread { offerTileOnce() }

    /** True while Android gives the recording silence instead of the
     * microphone, e.g. during a phone or internet call: only the call app
     * gets the microphone then (emulator, Android 15, 2026-11-23: from
     * accepting the call to hanging up, not while it rings). The page asks
     * with every clock tick while it records.
     *
     * Android hands apps every app's recordings without saying whose, so
     * another app's silenced recording counts too. No real case of that yet. */
    @JavascriptInterface
    fun silenced(): Boolean {
      if (Build.VERSION.SDK_INT < 29) return false
      return audio.activeRecordingConfigurations.any { it.isClientSilenced }
    }
  }

  private fun offerTileOnce() {
    if (Build.VERSION.SDK_INT < 33) return
    val prefs = getSharedPreferences("sokki", MODE_PRIVATE)
    if (prefs.getBoolean("tile_offered", false)) return
    val bar = getSystemService(StatusBarManager::class.java) ?: return
    bar.requestAddTileService(
      ComponentName(this, RecordTile::class.java),
      getString(R.string.tile_record),
      Icon.createWithResource(this, R.drawable.ic_tile_record),
      mainExecutor,
    ) { result ->
      Log.i("Sokki", "Tile offer: result $result")
      // Shown (added or not) or already there: done. An error (another
      // dialog in front, say) leaves it for the next start.
      val answered = result == StatusBarManager.TILE_ADD_REQUEST_RESULT_TILE_ADDED ||
        result == StatusBarManager.TILE_ADD_REQUEST_RESULT_TILE_NOT_ADDED ||
        result == StatusBarManager.TILE_ADD_REQUEST_RESULT_TILE_ALREADY_ADDED
      if (answered) prefs.edit().putBoolean("tile_offered", true).apply()
    }
  }

  private val audio by lazy { getSystemService(AudioManager::class.java) }

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

    /** The phone's own 12/24-hour setting, for times in the recording list. */
    @JavascriptInterface
    fun uses24Hour(): Boolean = android.text.format.DateFormat.is24HourFormat(this@MainActivity)
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
