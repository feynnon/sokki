// Sokki: the typing test. Shows one sentence at a time; you type it with the
// Sokki keyboard as you normally would. TypingTest records the taps. Open it
// from the keyboard settings ("Typing test") or with
//   adb shell am start -n dev.verbatim.mobile/dev.verbatim.sokki.TypingTestActivity
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

import android.app.Activity
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.text.InputType
import android.util.TypedValue
import android.view.Gravity
import android.view.KeyEvent
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.view.WindowManager
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.Switch
import android.widget.TextView
import androidx.core.content.edit
import helium314.keyboard.latin.settings.Defaults
import helium314.keyboard.latin.settings.Settings
import helium314.keyboard.latin.utils.prefs

class TypingTestActivity : Activity() {
    private val sentences = TypingTest.SENTENCES
    private var index = 0
    private lateinit var counter: TextView
    private lateinit var target: TextView
    private lateinit var field: EditText
    private lateinit var next: Button

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val night = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        val bg = if (night) 0xFF17181B.toInt() else 0xFFF7F7F8.toInt()
        val card = if (night) 0xFF1F2024.toInt() else Color.WHITE
        val line = if (night) 0xFF2C2E33.toInt() else 0xFFE1E3E7.toInt()
        val ink = if (night) 0xFFECEDEF.toInt() else 0xFF111214.toInt()
        val ink3 = 0xFF8D929B.toInt()
        val blue = 0xFF3567CC.toInt()
        window.statusBarColor = bg
        window.navigationBarColor = bg
        window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE
            or WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)

        val prefs = prefs()
        val targeting = prefs.getBoolean(Settings.PREF_SOKKI_KEY_TARGETING, Defaults.PREF_SOKKI_KEY_TARGETING)
        TypingTest.start(this, targeting)

        fun dp(v: Int) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), resources.displayMetrics).toInt()
        fun text(size: Float, color: Int, bold: Boolean = false) = TextView(this).apply {
            setTextSize(TypedValue.COMPLEX_UNIT_SP, size); setTextColor(color)
            if (bold) typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
        }

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(bg)
            setPadding(dp(20), dp(16), dp(20), dp(16))
        }
        root.addView(text(22f, ink, bold = true).apply { setText("Typing test") })
        root.addView(text(14f, ink3).apply {
            setText("Type each sentence the way you normally type. Fix mistakes if you notice them. Only your taps in this field are recorded.")
            setPadding(0, dp(6), 0, dp(16))
        })
        counter = text(13f, ink3)
        root.addView(counter)
        target = text(19f, ink).apply { setPadding(0, dp(6), 0, dp(12)); setLineSpacing(0f, 1.2f) }
        root.addView(target)
        field = EditText(this).apply {
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 18f)
            setTextColor(ink)
            setHintTextColor(ink3)
            background = GradientDrawable().apply { setColor(card); setStroke(dp(1), line); cornerRadius = dp(10).toFloat() }
            setPadding(dp(14), dp(12), dp(14), dp(12))
            // autocorrect on, as in a message field: the test shows what you get (the taps stay raw)
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES or InputType.TYPE_TEXT_FLAG_AUTO_CORRECT
            imeOptions = EditorInfo.IME_ACTION_NEXT or EditorInfo.IME_FLAG_NO_EXTRACT_UI
            privateImeOptions = "dev.verbatim.sokki.typingTest"
            isSingleLine = true
            setOnEditorActionListener { _, actionId, event ->
                if (actionId == EditorInfo.IME_ACTION_NEXT || event?.keyCode == KeyEvent.KEYCODE_ENTER) { advance(); true } else false
            }
        }
        root.addView(field, LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT))

        val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL; setPadding(0, dp(12), 0, 0) }
        val toggle = Switch(this).apply {
            setText("Smart key edges")
            setTextColor(ink3)
            isChecked = targeting
            setOnCheckedChangeListener { _, on ->
                prefs.edit { putBoolean(Settings.PREF_SOKKI_KEY_TARGETING, on) }
                TypingTest.note("keyTargeting", on)
            }
        }
        row.addView(toggle, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        val record = Switch(this).apply {
            setText("Record my taps in all apps (stays on this phone, never in password fields)")
            setTextColor(ink3)
            isChecked = prefs.getBoolean(Settings.PREF_SOKKI_RECORD_TAPS, Defaults.PREF_SOKKI_RECORD_TAPS)
            setPadding(0, dp(12), 0, 0)
            setOnCheckedChangeListener { _, on -> prefs.edit { putBoolean(Settings.PREF_SOKKI_RECORD_TAPS, on) } }
        }
        next = Button(this).apply {
            setText("Next")
            setTextColor(Color.WHITE)
            isAllCaps = false
            background = GradientDrawable().apply { setColor(blue); cornerRadius = dp(10).toFloat() }
            setPadding(dp(20), 0, dp(20), 0)
            setOnClickListener { advance() }
        }
        row.addView(next, LinearLayout.LayoutParams(WRAP_CONTENT, dp(44)))
        root.addView(row)
        root.addView(record, LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT))
        root.addView(Switch(this).apply {
            setText("Keep a journal of everything I write (stays on this phone, never passwords)")
            setTextColor(ink3)
            isChecked = Journal.isOn(this@TypingTestActivity)
            setPadding(0, dp(12), 0, 0)
            setOnCheckedChangeListener { _, on -> Journal.setOn(this@TypingTestActivity, on) }
        }, LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT))
        // the outer frame takes the status bar inset, so the padding above stays
        setContentView(android.widget.FrameLayout(this).apply {
            setBackgroundColor(bg)
            fitsSystemWindows = true
            addView(root)
        })
        show()
    }

    private fun show() {
        counter.text = "${index + 1} / ${sentences.size}"
        target.text = sentences[index]
        field.setText("")
        TypingTest.sentence(index, sentences[index], null)
        field.requestFocus()
        field.post {
            (getSystemService(INPUT_METHOD_SERVICE) as android.view.inputmethod.InputMethodManager)
                .showSoftInput(field, 0)
        }
    }

    private fun advance() {
        if (index >= sentences.size) { finish(); return }
        TypingTest.sentence(index, sentences[index], field.text.toString())
        index++
        if (index < sentences.size) { show(); return }
        TypingTest.finish()
        counter.text = "Done"
        target.text = "Thank you. ${sentences.size} sentences saved."
        field.isEnabled = false
        next.text = "Close"
    }

    override fun onResume() {
        super.onResume()
        TypingTest.active = index < sentences.size
    }

    override fun onPause() {
        super.onPause()
        TypingTest.active = false
    }

    override fun onDestroy() {
        super.onDestroy()
        if (isFinishing) TypingTest.finish()
    }
}
