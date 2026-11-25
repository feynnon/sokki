// The developer page "Keep what I write" (src/JournalView.tsx) reads the
// keyboard's journal through this: switch, list of days, one day's text,
// delete. The keyboard (another process) writes the files
// (mobile/keyboard/.../dev/verbatim/sokki/Journal.kt); nothing here sends
// anything. Sending to the Mac is the Rust command link_send_journal.
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.mobile

import android.content.Context
import android.webkit.JavascriptInterface
import dev.verbatim.sokki.Journal
import helium314.keyboard.latin.settings.Defaults
import helium314.keyboard.latin.settings.DebugSettings
import helium314.keyboard.latin.utils.prefs
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

class JournalBridge(private val context: Context) {
  private val dir get() = File(context.filesDir, "typing_log")
  private val name = Regex("""journal-\d{4}-\d{2}-\d{2}\.jsonl""")

  private fun files(): List<File> =
    dir.listFiles()?.filter { it.isFile && name.matches(it.name) }?.sortedByDescending { it.name } ?: emptyList()

  /** The page is for developers: shown in debug builds, after the keyboard's
   * debug settings are switched on (About, tap the version), or while the
   * journal is on. A store user doesn't come across it. */
  @JavascriptInterface fun available(): Boolean =
    (context.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0 ||
      context.prefs().getBoolean(DebugSettings.PREF_SHOW_DEBUG_SETTINGS, Defaults.PREF_SHOW_DEBUG_SETTINGS) ||
      Journal.isOn(context)

  @JavascriptInterface fun isOn(): Boolean = Journal.isOn(context)

  @JavascriptInterface fun setOn(on: Boolean) = Journal.setOn(context, on)

  /** Newest first: [{file, entries, bytes}]. */
  @JavascriptInterface fun days(): String {
    val out = JSONArray()
    for (f in files()) {
      val entries = f.useLines { lines -> lines.count { it.contains("\"t\":\"entry\"") } }
      out.put(JSONObject().put("file", f.name).put("entries", entries).put("bytes", f.length()))
    }
    return out.toString()
  }

  @JavascriptInterface fun read(file: String): String {
    if (!name.matches(file)) return ""
    return File(dir, file).takeIf { it.exists() }?.readText() ?: ""
  }

  /** Deletes every day of the journal; returns how many files. */
  @JavascriptInterface fun deleteAll(): Int = files().count { it.delete() }
}
