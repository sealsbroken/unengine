package app.onym.un

import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import uniffi.un_ffi.*
import java.util.Locale
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** The tutor's voice through Android TextToSpeech. `speak` blocks the engine's effect thread until done. */
class AndroidSpeechOutput(context: Context) : SpeechOutputCallback {
    private var ready = CountDownLatch(1)
    private val tts = TextToSpeech(context) { ready.countDown() }
    @Volatile private var done = CountDownLatch(0)

    init {
        tts.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
            override fun onStart(id: String?) {}
            override fun onDone(id: String?) { done.countDown() }
            @Deprecated("Deprecated in Java") override fun onError(id: String?) { done.countDown() }
        })
    }

    override fun prepare(language: UnLanguage) { ready.await(5, TimeUnit.SECONDS) }

    override fun speak(text: String, language: UnLanguage) {
        tts.language = if (language == UnLanguage.RUSSIAN) Locale("ru", "RU") else Locale.US
        tts.setSpeechRate(1.0f) // the voice's own pace
        tts.setPitch(1.45f) // a child's voice: there is no child voice, so the pitch goes up
        done = CountDownLatch(1)
        tts.speak(text, TextToSpeech.QUEUE_FLUSH, null, "un-${System.nanoTime()}")
        done.await(30, TimeUnit.SECONDS)
    }

    override fun stopSpeaking() { tts.stop(); done.countDown() }
    override fun playChime() {}
}

/** Spoken numbers through Android's SpeechRecognizer (on-device where available). */
class AndroidSpeechInput(private val context: Context) : SpeechInputCallback {
    private val main = Handler(Looper.getMainLooper())
    private var recognizer: SpeechRecognizer? = null
    @Volatile private var cancelled = false

    /** The recogniser is closed while the tutor speaks (no barge-in on Android yet). */
    override fun bargeIn() = false

    override fun prepare(language: UnLanguage) {
        if (!SpeechRecognizer.isRecognitionAvailable(context)) throw UnException.Failed("speech recognition unavailable")
    }

    override fun listenForNumber(language: UnLanguage, timeoutSeconds: Double): UnHeard? {
        var heard: UnHeard? = null
        listen(language, timeoutSeconds) { text ->
            NumberWords.parse(text, language)?.let { heard = UnHeard(it, text) } != null
        }
        return heard
    }

    override fun listenForText(language: UnLanguage, timeoutSeconds: Double): String? =
        listen(language, timeoutSeconds) { false }

    /// Recognises until `done` accepts a partial transcript or the recogniser delivers its final
    /// result (Android ends the utterance itself when the child pauses); the transcript, or null
    /// when nothing was said before the timeout or listening was cancelled.
    private fun listen(language: UnLanguage, timeoutSeconds: Double, done: (String) -> Boolean): String? {
        cancelled = false
        val latch = CountDownLatch(1)
        var transcript: String? = null
        val locale = if (language == UnLanguage.RUSSIAN) "ru-RU" else "en-US"
        main.post {
            val r = SpeechRecognizer.createSpeechRecognizer(context).also { recognizer = it }
            r.setRecognitionListener(object : RecognitionListener {
                override fun onResults(results: Bundle?) {
                    val texts = results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION).orEmpty()
                    texts.firstOrNull { it.isNotBlank() }?.let { transcript = it }
                    latch.countDown()
                }
                override fun onPartialResults(partial: Bundle?) {
                    val texts = partial?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION).orEmpty()
                    for (text in texts) if (text.isNotBlank()) {
                        transcript = text
                        if (done(text)) latch.countDown()
                    }
                }
                override fun onError(error: Int) { latch.countDown() }
                override fun onReadyForSpeech(p: Bundle?) {}
                override fun onBeginningOfSpeech() {}
                override fun onRmsChanged(rms: Float) {}
                override fun onBufferReceived(b: ByteArray?) {}
                override fun onEndOfSpeech() {}
                override fun onEvent(e: Int, p: Bundle?) {}
            })
            r.startListening(Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
                putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
                putExtra(RecognizerIntent.EXTRA_LANGUAGE, locale)
                putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
                putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
            })
        }
        latch.await((timeoutSeconds * 1000).toLong(), TimeUnit.MILLISECONDS)
        main.post { recognizer?.destroy(); recognizer = null }
        return if (cancelled) null else transcript?.trim()?.takeIf { it.isNotEmpty() }
    }

    override fun cancelListening() {
        cancelled = true
        main.post { recognizer?.cancel() }
    }
}

/** A small forgiving number parser mirroring `un_core::NumberParser` for the Android recogniser path. */
object NumberWords {
    private val english = mapOf("zero" to 0u, "none" to 0u, "nothing" to 0u, "one" to 1u, "won" to 1u, "two" to 2u, "to" to 2u, "too" to 2u,
        "three" to 3u, "tree" to 3u, "free" to 3u, "four" to 4u, "for" to 4u, "five" to 5u, "six" to 6u, "seven" to 7u, "eight" to 8u,
        "ate" to 8u, "nine" to 9u, "ten" to 10u, "eleven" to 11u, "twelve" to 12u, "thirteen" to 13u, "fourteen" to 14u, "fifteen" to 15u,
        "sixteen" to 16u, "seventeen" to 17u, "eighteen" to 18u, "nineteen" to 19u, "twenty" to 20u)
    private val russian = mapOf("ноль" to 0u, "нет" to 0u, "ничего" to 0u, "один" to 1u, "одна" to 1u, "одно" to 1u, "два" to 2u, "две" to 2u,
        "три" to 3u, "четыре" to 4u, "пять" to 5u, "шесть" to 6u, "семь" to 7u, "восемь" to 8u, "девять" to 9u, "десять" to 10u,
        "одиннадцать" to 11u, "двенадцать" to 12u, "тринадцать" to 13u, "четырнадцать" to 14u, "пятнадцать" to 15u, "шестнадцать" to 16u,
        "семнадцать" to 17u, "восемнадцать" to 18u, "девятнадцать" to 19u, "двадцать" to 20u)

    /** The last number in the transcript, like the engine's parser. */
    fun parse(text: String, language: UnLanguage): UInt? {
        val table = if (language == UnLanguage.RUSSIAN) russian else english
        var last: UInt? = null
        for (token in text.lowercase().replace('ё', 'е').split(Regex("[^\\p{L}\\p{N}]+")).filter { it.isNotEmpty() }) {
            token.toUIntOrNull()?.let { if (it <= 20u) last = it } ?: table[token]?.let { last = it }
        }
        return last
    }
}
