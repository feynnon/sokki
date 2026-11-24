package dev.verbatim.mobile

import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.webkit.WebView
import java.lang.ref.WeakReference

/** "Record meeting" in Android's Quick Settings panel (swipe down, edit,
 * drag it in).
 *
 * Tapping it opens Sokki straight into recording, the same recording as the
 * app's own button. It can't record without opening Sokki: Android gives an
 * app the microphone only while it is on screen, or from a foreground service
 * started while it was, and a tile tap counts as neither. Tested on Android
 * 15, 2026-11-23: the tap lets the app start a foreground service ("tile
 * onclick" allowance), but one of type microphone throws a SecurityException
 * ("Foreground service started from background can not have
 * location/camera/microphone access").
 *
 * Tapping it again while Sokki records stops the recording without opening
 * anything; the recording is then sent to the Mac like any other.
 *
 * During a phone call Android gives Sokki silence (see mobile/README.md). */
class RecordTile : TileService() {
  override fun onStartListening() = show(Recording.on)

  override fun onClick() {
    if (Recording.on && Recording.stop()) {
      show(false)
      return
    }
    if (isLocked) unlockAndRun { openToRecord() } else openToRecord()
  }

  private fun openToRecord() {
    val intent = Intent(this, MainActivity::class.java)
      .setAction(MainActivity.ACTION_RECORD)
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    if (Build.VERSION.SDK_INT >= 34) {
      val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
      startActivityAndCollapse(PendingIntent.getActivity(this, 0, intent, flags))
    } else {
      @Suppress("DEPRECATION")
      startActivityAndCollapse(intent)
    }
  }

  private fun show(on: Boolean) {
    val tile = qsTile ?: return
    tile.state = if (on) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
    if (Build.VERSION.SDK_INT >= 29) {
      tile.subtitle = getString(if (on) R.string.tile_recording else R.string.app_name)
    }
    tile.updateTile()
  }
}

/** Whether the page is recording a meeting, as the page reports it
 * (`window.SokkiRecorder.recording`). The tile reads it each time the panel
 * opens. */
object Recording {
  @Volatile var on = false
  @Volatile var page: WeakReference<WebView>? = null

  /** Stops the page's recording. False when the page is gone. */
  fun stop(): Boolean {
    val web = page?.get() ?: return false
    web.post { web.evaluateJavascript("window.__sokkiRecord && window.__sokkiRecord('stop')", null) }
    return true
  }
}
