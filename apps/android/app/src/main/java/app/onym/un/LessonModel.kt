package app.onym.un

import android.content.Context
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import kotlinx.coroutines.flow.MutableStateFlow
import uniffi.un_ffi.*
import java.io.File
import java.util.Timer
import java.util.TimerTask

/** Wires CameraX frames into the shared perception and the lesson into the shared engine. */
class LessonModel(private val context: Context) {
    val snapshot = MutableStateFlow<UnSnapshot?>(null)
    val running = MutableStateFlow(false)
    private var session: UnSession? = null
    private var perception: UnPerception? = null
    private var timer: Timer? = null
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
        val config = defaultSessionConfig().copy(language = language.value)
        perception = UnPerception(UnPerceptionConfig(modelsDir, "detector", 0.25f, true, null, false, null))
        session = UnSession(availableStrategies().firstOrNull()?.id ?: "demo", config, AndroidSpeechInput(context), AndroidSpeechOutput(context), true)
        startCamera()
        session?.start()
        running.value = true
        timer = Timer().apply { schedule(object : TimerTask() { override fun run() { snapshot.value = session?.snapshot() } }, 0, 100) }
    }

    private fun startCamera() {
        val future = ProcessCameraProvider.getInstance(context)
        future.addListener({
            val provider = future.get()
            val analysis = ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
            analysis.setAnalyzer(ContextCompat.getMainExecutor(context)) { image -> analyze(image) }
            provider.unbindAll()
            provider.bindToLifecycle(context as LifecycleOwner, androidx.camera.core.CameraSelector.DEFAULT_BACK_CAMERA, analysis)
        }, ContextCompat.getMainExecutor(context))
    }

    private fun analyze(image: ImageProxy) {
        image.use {
            val y = it.planes[0]; val uv = it.planes[1]   // YUV_420_888; plane 1 interleaved on most devices (NV12/NV21-like)
            val yBytes = ByteArray(y.buffer.remaining()).also { b -> y.buffer.get(b) }
            val uvBytes = ByteArray(uv.buffer.remaining() + 1).also { b -> uv.buffer.get(b, 0, uv.buffer.remaining()) }
            val t = (System.nanoTime() - started) / 1e9
            val state = perception?.processNv12(yBytes, uvBytes, it.width.toUInt(), it.height.toUInt(), y.rowStride.toUInt(), uv.rowStride.toUInt(), t)
            if (state != null) session?.observe(state.count, state.handsPresent, state.isStable, state.fingerCount, state.groups, state.pointedAt)
        }
    }

    fun stop() { session?.stop(); running.value = false }
    fun tap(n: UInt) { session?.tap(n) }
    fun repeat() { session?.repeat() }
    fun skip() { session?.skip() }

    fun close() {
        timer?.cancel()
        session?.close()
        session = null
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
