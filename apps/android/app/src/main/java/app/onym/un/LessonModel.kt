package app.onym.un

import android.content.Context
import android.view.Surface
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import kotlinx.coroutines.flow.MutableStateFlow
import uniffi.un_ffi.*
import java.io.File
import java.util.Timer
import java.util.TimerTask

/**
 * Wires CameraX frames into the shared perception and the lesson into the shared engine, as the
 * iOS `LessonModel` does: the toys detector with hand tracking, lesson 1 when it is registered,
 * the camera's picture behind the overlay, the language remembered between runs.
 */
class LessonModel(private val context: Context) {
    val snapshot = MutableStateFlow<UnSnapshot?>(null)
    val running = MutableStateFlow(false)
    /** The latest table state and the size of the frame it came from, for the overlay. */
    val table = MutableStateFlow<UnTableState?>(null)
    val frameSize = MutableStateFlow(Pair(0, 0))
    /** The camera's own picture, filled to the view keeping the frame's aspect (like iOS). */
    val previewView: PreviewView by lazy { PreviewView(context).apply { scaleType = PreviewView.ScaleType.FILL_CENTER } }
    private var session: UnSession? = null
    private var perception: UnPerception? = null
    private var timer: Timer? = null
    private var cameraStarted = false
    private val started = System.nanoTime()
    private val prefs = context.getSharedPreferences("un", Context.MODE_PRIVATE)
    /** The tutor's language, remembered between runs; the device's language the first time. */
    val language = MutableStateFlow(savedLanguage())

    private fun savedLanguage(): UnLanguage = when (prefs.getString("language", null)) {
        "ru" -> UnLanguage.RUSSIAN
        "en" -> UnLanguage.ENGLISH
        else -> if (java.util.Locale.getDefault().language == "ru") UnLanguage.RUSSIAN else UnLanguage.ENGLISH
    }

    /** Switches the tutor's language; a running lesson starts over in the new one. */
    fun setLanguage(l: UnLanguage) {
        if (l == language.value) return
        language.value = l
        prefs.edit().putString("language", if (l == UnLanguage.RUSSIAN) "ru" else "en").apply()
        if (running.value) { close(); start() }
    }

    fun start() {
        val modelsDir = ModelAssets.unpack(context)   // copies assets/models to filesDir once
        var config = defaultSessionConfig().copy(language = language.value)
        if (BuildConfig.DEBUG) {
            // As on iOS: a debug build skips the introductions and goes straight to the lesson.
            config = config.copy(childName = if (language.value == UnLanguage.RUSSIAN) "Вася" else "Mia")
        }
        perception = UnPerception(UnPerceptionConfig(modelsDir, "toys", 0.25f, true, null, false, null))
        val strategies = availableStrategies()
        val lesson = strategies.firstOrNull { it.id == "lesson-01" } ?: strategies.firstOrNull()
        session = UnSession(lesson?.id ?: "demo", config, AndroidSpeechInput(context), AndroidSpeechOutput(context), true)
        startCamera()
        session?.start()
        running.value = true
        timer = Timer().apply { schedule(object : TimerTask() { override fun run() { snapshot.value = session?.snapshot() } }, 0, 100) }
    }

    private fun startCamera() {
        if (cameraStarted) return
        cameraStarted = true
        val future = ProcessCameraProvider.getInstance(context)
        future.addListener({
            val provider = future.get()
            val preview = Preview.Builder().build().also { it.surfaceProvider = previewView.surfaceProvider }
            // Frames come out upright (portrait, like the preview), so the overlay maps 1:1.
            val analysis = ImageAnalysis.Builder()
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .setOutputImageRotationEnabled(true)
                .setTargetRotation(Surface.ROTATION_0)
                .build()
            analysis.setAnalyzer(ContextCompat.getMainExecutor(context)) { image -> analyze(image) }
            provider.unbindAll()
            provider.bindToLifecycle(context as LifecycleOwner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
        }, ContextCompat.getMainExecutor(context))
    }

    private fun analyze(image: ImageProxy) {
        image.use {
            val y = it.planes[0]; val uv = it.planes[1]   // YUV_420_888; plane 1 interleaved on most devices (NV12/NV21-like)
            val yBytes = ByteArray(y.buffer.remaining()).also { b -> y.buffer.get(b) }
            val uvBytes = ByteArray(uv.buffer.remaining() + 1).also { b -> uv.buffer.get(b, 0, uv.buffer.remaining()) }
            val t = (System.nanoTime() - started) / 1e9
            val state = perception?.processNv12(yBytes, uvBytes, it.width.toUInt(), it.height.toUInt(), y.rowStride.toUInt(), uv.rowStride.toUInt(), t)
            if (state != null) {
                session?.observe(state.count, state.handsPresent, state.isStable, state.fingerCount, state.groups, state.pointedAt)
                table.value = state
                frameSize.value = Pair(it.width, it.height)
            }
        }
    }

    fun stop() { session?.stop(); running.value = false }
    fun tap(n: UInt) { session?.tap(n) }
    fun repeat() { session?.repeat() }
    fun skip() { session?.skip() }

    fun close() {
        timer?.cancel()
        session?.shutdown()
        session = null
        table.value = null
    }
}

object ModelAssets {
    fun unpack(context: Context): String {
        val target = File(context.filesDir, "models")
        if (!target.exists()) {
            target.mkdirs()
            context.assets.list("models")?.forEach { name ->
                context.assets.open("models/$name").use { input -> File(target, name).outputStream().use { input.copyTo(it) } }
            }
        }
        return target.absolutePath
    }
}
