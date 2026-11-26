// "Note ready": the phone tells you when the Mac has finished a meeting,
// also while Sokki is closed (Robin's version 2, 26.11.2026, design canvas
// "Handy: Meetings").
//
// The page reports each recording the Mac has taken (`window.SokkiNotes.watch`)
// and each note it has fetched itself (`seen`). For the rest, Android runs
// NoteJob about every 15 minutes (the shortest interval Android allows a
// background job) while any are open: it asks the Mac over the phone link
// (Rust: src-tauri/src/note_check.rs) and shows a notification for each
// finished note, with Share and Copy summary. Nothing runs when no note is
// open, and nothing at all without a Mac.
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.mobile

import android.Manifest
import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import android.webkit.JavascriptInterface
import org.json.JSONArray
import org.json.JSONObject

object NoteCheck {
  private const val TAG = "SokkiNotes"
  private const val PREFS = "note_check"
  private const val IDS = "ids"
  private const val JOB = 7301
  private const val CHANNEL = "notes"
  /** Also checked right away in tests: `adb shell cmd jobscheduler run -f <pkg> 7301`. */
  private const val EVERY_MS = 15 * 60 * 1000L

  @JvmStatic private external fun nativeStatus(dataDir: String, ids: String): String?

  private var loaded = false
  private fun load(): Boolean {
    if (!loaded) loaded = runCatching { System.loadLibrary("mobile_lib") }.isSuccess
    return loaded
  }

  private fun ids(ctx: Context): Set<String> =
    ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getStringSet(IDS, emptySet()) ?: emptySet()

  private fun setIds(ctx: Context, ids: Set<String>) {
    ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putStringSet(IDS, ids).apply()
    val jobs = ctx.getSystemService(JobScheduler::class.java)
    if (ids.isEmpty()) {
      jobs.cancel(JOB)
    } else if (jobs.getPendingJob(JOB) == null) {
      jobs.schedule(
        JobInfo.Builder(JOB, ComponentName(ctx, NoteJob::class.java))
          .setPeriodic(EVERY_MS)
          .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
          .build(),
      )
    }
  }

  fun watch(ctx: Context, id: String) = setIds(ctx, ids(ctx) + id)

  fun seen(ctx: Context, id: String) {
    setIds(ctx, ids(ctx) - id)
    ctx.getSystemService(NotificationManager::class.java).cancel(id.hashCode())
  }

  /** One round: ask the Mac about every open note. Runs on the job's thread. */
  fun check(ctx: Context) {
    val open = ids(ctx)
    if (open.isEmpty() || !load()) return
    val reply = nativeStatus(ctx.dataDir.path, JSONArray(open.toList()).toString()) ?: return
    val answers = JSONArray(reply)
    var left = open
    var errors = 0
    for (i in 0 until answers.length()) {
      val a = answers.getJSONObject(i)
      val id = a.optString("id")
      when (a.optString("status")) {
        "done" -> {
          notifyReady(ctx, id, a)
          left = left - id
        }
        "failed" -> left = left - id
        "" -> errors++
      }
    }
    // Lengths and states only: what was said stays out of the log.
    Log.i(TAG, "checked ${open.size}: ${open.size - left.size} finished, $errors unreachable")
    setIds(ctx, left)
  }

  private fun notifyReady(ctx: Context, id: String, note: JSONObject) {
    val nm = ctx.getSystemService(NotificationManager::class.java)
    nm.createNotificationChannel(
      NotificationChannel(CHANNEL, "Meeting notes", NotificationManager.IMPORTANCE_DEFAULT).apply {
        description = "When Sokki on your Mac has finished a meeting you recorded"
      },
    )
    val title = note.optString("title").ifBlank { "Your meeting" }
    val summary = plain(note.optString("summary"))
    val first = summary.lines().firstOrNull { it.startsWith("• ") }?.removePrefix("• ") ?: ""
    val tasks = summary.lines().count { it.startsWith("☐ ") }
    val body = listOf(first, if (tasks > 0) "$tasks ${if (tasks == 1) "task" else "tasks"}." else "")
      .filter { it.isNotBlank() }.joinToString(" ")
    val share = "$title\n\n$summary"
    val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    val open = PendingIntent.getActivity(
      ctx, id.hashCode(),
      Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK), flags,
    )
    val shareIt = PendingIntent.getActivity(
      ctx, id.hashCode() + 1,
      Intent.createChooser(
        Intent(Intent.ACTION_SEND).setType("text/plain")
          .putExtra(Intent.EXTRA_SUBJECT, title).putExtra(Intent.EXTRA_TEXT, share),
        null,
      ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
      flags,
    )
    val copyIt = PendingIntent.getBroadcast(
      ctx, id.hashCode() + 2,
      Intent(ctx, NoteCopy::class.java).putExtra("text", share).putExtra("id", id), flags,
    )
    val n = Notification.Builder(ctx, CHANNEL)
      .setSmallIcon(R.drawable.ic_tile_record)
      .setContentTitle("Note ready: $title")
      .setContentText(body)
      .setStyle(Notification.BigTextStyle().bigText(body))
      .setContentIntent(open)
      .setAutoCancel(true)
      .addAction(Notification.Action.Builder(null, "Share", shareIt).build())
      .addAction(Notification.Action.Builder(null, "Copy summary", copyIt).build())
      .build()
    nm.notify(id.hashCode(), n)
  }

  /** The Mac's summary (Markdown) as plain text for sharing and pasting:
   * "## Tasks" → "Tasks", "- [ ] x" → "☐ x", "- x" → "• x", no "**". */
  fun plain(md: String): String =
    md.lines().joinToString("\n") { raw ->
      val l = raw.trimEnd().replace("**", "")
      when {
        l.startsWith("## ") -> l.removePrefix("## ")
        l.startsWith("- [ ] ") -> "☐ " + l.removePrefix("- [ ] ")
        l.startsWith("- [x] ") -> "☑ " + l.removePrefix("- [x] ")
        l.startsWith("- ") -> "• " + l.removePrefix("- ")
        else -> l
      }
    }.trim()

  /** Android 13+ asks once whether Sokki may show notifications. */
  fun askPermission(activity: Activity) {
    if (Build.VERSION.SDK_INT < 33) return
    if (activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED) return
    activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 3)
  }
}

class NoteJob : JobService() {
  override fun onStartJob(params: JobParameters): Boolean {
    Thread {
      runCatching { NoteCheck.check(applicationContext) }
      jobFinished(params, false)
    }.start()
    return true
  }

  override fun onStopJob(params: JobParameters) = true
}

/** "Copy summary" in the notification. */
class NoteCopy : BroadcastReceiver() {
  override fun onReceive(ctx: Context, intent: Intent) {
    val text = intent.getStringExtra("text") ?: return
    ctx.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Sokki note", text))
    intent.getStringExtra("id")?.let { ctx.getSystemService(NotificationManager::class.java).cancel(it.hashCode()) }
  }
}

/** `window.SokkiNotes` for the page (src/App.tsx). */
class NotesBridge(private val activity: Activity) {
  @JavascriptInterface fun watch(id: String) = NoteCheck.watch(activity, id)

  /** Only while Sokki is in front: when Android wakes the app for NoteJob,
   * the page's own checks run too, and "Note ready" must stay then. */
  @JavascriptInterface fun seen(id: String) {
    val front = (activity as? androidx.lifecycle.LifecycleOwner)
      ?.lifecycle?.currentState?.isAtLeast(androidx.lifecycle.Lifecycle.State.RESUMED) ?: false
    if (front) NoteCheck.seen(activity, id)
  }

  /** Android's clipboard: the WebView's own is refused (no secure context). */
  @JavascriptInterface fun copy(text: String) {
    activity.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Sokki note", text))
  }

  @JavascriptInterface fun askPermission() = activity.runOnUiThread { NoteCheck.askPermission(activity) }

  /** Android's share sheet with the note's title as subject (mail) and text. */
  @JavascriptInterface fun share(title: String, text: String) = activity.runOnUiThread {
    val send = Intent(Intent.ACTION_SEND).setType("text/plain")
      .putExtra(Intent.EXTRA_SUBJECT, title).putExtra(Intent.EXTRA_TEXT, text)
    activity.startActivity(Intent.createChooser(send, null))
  }
}
