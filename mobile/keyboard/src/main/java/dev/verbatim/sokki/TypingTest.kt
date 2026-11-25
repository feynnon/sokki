// Sokki: records taps while the typing test (TypingTestActivity) is open, to
// learn the user's own typing pattern and to replay it offline with and
// without letter-aware key edges (mobile/keyboard/tools/typing_eval.py).
//
// Records only inside the test's own field: where each tap landed, which key
// geometry gave, which key was sent, the keyboard layout, and the test
// sentences with what was typed. Nothing else, nothing outside the test.
// Files: <app files>/typing_test/test-<time>.jsonl (adb: run-as dev.verbatim.mobile).
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

import android.content.Context
import android.os.Build
import android.os.SystemClock
import android.util.Log
import helium314.keyboard.keyboard.Key
import helium314.keyboard.keyboard.KeyDetector
import helium314.keyboard.keyboard.Keyboard
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.Writer
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

object TypingTest {
    private const val TAG = "SokkiTypingTest"

    /** True while the test screen is in front (same process as the keyboard). */
    @Volatile @JvmField var active = false

    private var writer: Writer? = null
    private var lastKeyboard: Keyboard? = null
    var file: File? = null
        private set

    @Synchronized fun start(context: Context, keyTargeting: Boolean): File {
        finish()
        val dir = File(context.filesDir, "typing_test").apply { mkdirs() }
        val name = "test-" + SimpleDateFormat("yyyyMMdd-HHmmss", Locale.ROOT).format(Date()) + ".jsonl"
        val f = File(dir, name)
        file = f
        writer = f.bufferedWriter()
        lastKeyboard = null
        val dm = context.resources.displayMetrics
        write(JSONObject()
            .put("t", "session")
            .put("device", Build.MODEL)
            .put("density", dm.density.toDouble())
            .put("xdpi", dm.xdpi.toDouble())
            .put("keyTargeting", keyTargeting))
        return f
    }

    @Synchronized fun sentence(index: Int, target: String, typed: String?) {
        write(JSONObject().put("t", if (typed == null) "sentence_start" else "sentence_end")
            .put("i", index).put("target", target).apply { if (typed != null) put("typed", typed) })
    }

    @Synchronized fun note(what: String, value: Any) {
        write(JSONObject().put("t", what).put("value", value))
    }

    @JvmStatic @Synchronized fun logTap(detector: KeyDetector, sent: Key, x: Int, y: Int) {
        if (writer == null) return
        val keyboard = detector.keyboard ?: return
        if (keyboard !== lastKeyboard) {
            lastKeyboard = keyboard
            write(layout(keyboard))
        }
        val tx = detector.getTouchX(x)
        val ty = detector.getTouchY(y)
        val geo = detector.detectGeometricKey(tx, ty)
        write(JSONObject()
            .put("t", "tap")
            .put("ms", SystemClock.uptimeMillis())
            .put("x", tx).put("y", ty)
            .put("geo", geo?.code ?: JSONObject.NULL)
            .put("out", sent.code))
    }

    fun layout(keyboard: Keyboard): JSONObject {
        val keys = JSONArray()
        for (k in keyboard.sortedKeys) {
            val b = k.hitBox
            keys.put(JSONObject()
                .put("c", k.code)
                .put("l", k.label ?: "")
                .put("x", b.left).put("y", b.top).put("w", b.width()).put("h", b.height())
                .put("vx", k.x).put("vy", k.y).put("vw", k.width).put("vh", k.height))
        }
        return JSONObject()
            .put("t", "layout")
            .put("element", keyboard.mId.element.name)
            .put("lang", keyboard.mId.locale.toLanguageTag())
            .put("width", keyboard.mOccupiedWidth)
            .put("height", keyboard.mOccupiedHeight)
            .put("keys", keys)
    }

    private fun write(o: JSONObject) {
        val w = writer ?: return
        try {
            w.write(o.toString()); w.write("\n"); w.flush()
        } catch (e: Exception) {
            Log.w(TAG, "write", e)
        }
    }

    @Synchronized fun finish() {
        try { writer?.close() } catch (_: Exception) {}
        writer = null
        active = false
    }

    /** Everyday German, only letters, spaces, commas and full stops. */
    val SENTENCES = listOf(
        "Ich bin in zehn Minuten da, fang schon mal an.",
        "Kannst du mir bitte die Folien von gestern schicken.",
        "Das Meeting am Donnerstag verschiebt sich auf halb drei.",
        "Schöne Grüße aus Köln, wir sehen uns nächste Woche.",
        "Ich habe den Zug verpasst und komme etwas später.",
        "Wir müssen noch klären, wer die Präsentation übernimmt.",
        "Danke für deine schnelle Antwort, das hilft mir sehr.",
        "Hast du heute Abend Zeit für ein kurzes Telefonat.",
        "Die Rechnung ist schon bezahlt, du musst nichts mehr tun.",
        "Ich schicke dir gleich die Adresse vom Restaurant.",
        "Können wir das Gespräch auf morgen früh verschieben.",
        "Bitte vergiss nicht, Milch und Brot mitzubringen.",
        "Der Kunde möchte bis Freitag ein Angebot haben.",
        "Ich finde die Idee gut, lass uns das ausprobieren.",
        "Mein Akku ist fast leer, ich melde mich später.",
        "Wir treffen uns um acht Uhr vor dem Bahnhof.",
        "Kannst du kurz schauen, ob die Tür abgeschlossen ist.",
        "Das neue Büro ist größer und viel heller als das alte.",
        "Ich habe die Zahlen geprüft, sie stimmen jetzt.",
        "Gute Besserung, ruh dich am Wochenende richtig aus.",
        "Wie lange brauchst du ungefähr für den Entwurf.",
        "Wir sollten die Kosten für das nächste Jahr planen.",
        "Ich bin gerade im Supermarkt, brauchst du noch etwas.",
        "Die Kinder schlafen schon, ruf lieber morgen an.",
        "Das Wetter ist schön, wollen wir draußen essen.",
        "Ich habe dir eine Mail mit allen Unterlagen geschickt.",
        "Leider schaffe ich es heute nicht mehr ins Büro.",
        "Herzlichen Glückwunsch zum Geburtstag, feier schön.",
        "Der Flug landet um viertel nach sechs in München.",
        "Kannst du bitte prüfen, ob der Vertrag unterschrieben ist.",
        "Wir brauchen noch zwei Stühle für den Besprechungsraum.",
        "Ich übernehme das Protokoll, du kümmerst dich um die Agenda.",
        "Die Straße ist gesperrt, nimm lieber die Umleitung.",
        "Vielen Dank für das schöne Wochenende bei euch.",
        "Bis später, ich freue mich schon auf das Essen.",
        "Kannst du den Termin mit dem Zahnarzt bestätigen.",
        "Wir haben noch Platz für eine weitere Person im Auto.",
        "Das Paket wurde gestern beim Nachbarn abgegeben.",
        "Ich höre mir die Aufnahme heute Abend in Ruhe an.",
        "Lass uns nächste Woche über die Ergebnisse sprechen.",
    )
}
