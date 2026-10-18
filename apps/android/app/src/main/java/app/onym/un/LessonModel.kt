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
 * iOS `LessonModel` does: the toys detector, the lesson picked from the list of lessons the
 * library offers, the camera's picture behind the overlay, the language and the last lesson
 * remembered between runs.
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
    /** Set from a background thread once the detector is loaded; frames wait until then. */
    @Volatile private var perception: UnPerception? = null
    /** The detector is still loading (as on iOS, shown to the grown-up). */
    val preparingCamera = MutableStateFlow(false)
    /** The detector did not load; the lesson runs without the camera's count. */
    val detectorFailed = MutableStateFlow(false)
    private var timer: Timer? = null
    private var cameraStarted = false
    private val started = System.nanoTime()
    private val prefs = context.getSharedPreferences("un", Context.MODE_PRIVATE)
    /** The tutor's language, remembered between runs; the device's language the first time. */
    val language = MutableStateFlow(savedLanguage())
    /** The lessons the library offers, in its order. */
    val lessons = MutableStateFlow<List<UnStrategyInfo>>(emptyList())
    /** The lesson last started. */
    val lessonId = MutableStateFlow(prefs.getString("lesson", "") ?: "")

    fun reloadLessons() { lessons.value = availableStrategies() }

    /** As on iOS: the children and their progress through the lessons. */
    val profiles = UnProfiles(File(context.filesDir, "profiles.json").path)
    val children = MutableStateFlow<List<UnProfile>>(emptyList())
    /** The child the lessons are for, remembered between runs; null shows the children. */
    val childId = MutableStateFlow(prefs.getString("child", null))
    @Volatile private var recorded = false

    fun reloadChildren() {
        children.value = profiles.list()
        if (children.value.none { it.id == childId.value }) chooseChild(null)
    }

    fun chooseChild(id: String?) {
        childId.value = id
        prefs.edit().putString("child", id).apply()
    }

    fun addChild(name: String) {
        if (name.isBlank()) return
        val added = profiles.add(name.trim())
        reloadChildren()
        chooseChild(added.id)
    }

    fun removeChild(id: String) { profiles.remove(id); reloadChildren() }

    /** The lesson `child` goes on with. */
    fun nextLesson(child: UnProfile): UnStrategyInfo? {
        val id = profiles.nextLesson(child.id)
        return lessons.value.firstOrNull { it.id == id }
    }

    /** Loads the detector in the background from the lesson list on, so a lesson starts with the camera counting. */
    fun preparePerception() {
        if (perception != null || preparingCamera.value) return
        preparingCamera.value = true
        detectorFailed.value = false
        Thread {
            val loaded = runCatching {
                UnPerception(UnPerceptionConfig(ModelAssets.unpack(context), "toys", 0.25f, null, null))
            }.getOrNull()
            perception = loaded
            detectorFailed.value = loaded == null
            preparingCamera.value = false
        }.start()
    }

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

    /** Starts lesson `id` for the chosen child (the one they go on with when null). */
    fun start(id: String? = null) {
        if (lessons.value.isEmpty()) reloadLessons()
        val child = childId.value
        if (id != null) lessonId.value = id
        else if (child != null) profiles.nextLesson(child)?.let { lessonId.value = it }
        val lesson = lessons.value.firstOrNull { it.id == lessonId.value } ?: lessons.value.firstOrNull() ?: return
        lessonId.value = lesson.id
        prefs.edit().putString("lesson", lesson.id).apply()
        var config = defaultSessionConfig().copy(language = language.value)
        // The child's name and what their lessons remembered: each child goes on where they stopped.
        if (child != null) config = profiles.sessionConfig(child, config)
        recorded = false
        preparePerception()
        session = UnSession(lesson.id, config, AndroidSpeechInput(context), AndroidSpeechOutput(context), true, true)
        startCamera()
        // As on iOS: after a completed lesson the tutor asks "Продолжим?"; a yes starts the next one.
        session?.offerNextLesson(nextLessonId() != null)
        session?.start()
        running.value = true
        timer = Timer().apply { schedule(object : TimerTask() { override fun run() {
            snapshot.value = session?.snapshot()
            // The lesson's end, recorded for the child once.
            val summary = snapshot.value?.summary
            val child = childId.value
            if (summary != null && !recorded && child != null) {
                recorded = true
                profiles.record(child, lessonId.value, summary)
                children.value = profiles.list()
            }
            // Paused: back to the list once the goodbye is said and the lesson recorded (at the latest after 8 s).
            val s = snapshot.value
            if (s != null && (s.pauseRequested || leavingSince != null)) {
                val since = leavingSince ?: System.nanoTime().also { leavingSince = it }
                val said = s.status == UnStatus.FINISHED && (recorded || child == null)
                if (said || System.nanoTime() - since > 8_000_000_000L) {
                    cancel()
                    android.os.Handler(android.os.Looper.getMainLooper()).post { end() }
                    return
                }
            }
            val next = child?.let { profiles.nextLesson(it) } ?: nextLessonId()
            if (snapshot.value?.nextLessonRequested == true && next != null) {
                cancel()
                android.os.Handler(android.os.Looper.getMainLooper()).post { close(); start(next) }
            }
        } }, 0, 100) }
    }

    /** The lesson after the current one in the list, if any. */
    private fun nextLessonId(): String? {
        val list = lessons.value
        val i = list.indexOfFirst { it.id == lessonId.value }
        return if (i >= 0 && i + 1 < list.size) list[i + 1].id else null
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
                session?.observe(state.count, state.isStable, state.groups)
                table.value = state
                frameSize.value = Pair(it.width, it.height)
            }
        }
    }

    fun stop() { session?.stop(); running.value = false }
    /** Ends the lesson and goes back to the list. */
    /** As on iOS: when a pause was asked for (the button or "пауза" said aloud). */
    @Volatile private var leavingSince: Long? = null

    /** Stops the lesson as paused and goes back to the list after the tutor's goodbye. */
    fun pause() {
        if (leavingSince != null) return end()
        leavingSince = System.nanoTime()
        session?.stop()
    }

    /** Ends the lesson at once and goes back to the list. */
    fun end() { leavingSince = null; close(); running.value = false; snapshot.value = null }
    fun tap(n: UInt) { session?.tap(n) }
    fun repeat() { session?.repeat() }
    fun skip() { session?.skip() }

    fun close() {
        timer?.cancel()
        // Torn down off the main thread: shutting down waits for the engine's threads.
        val closing = session
        session = null
        Thread { closing?.shutdown() }.start()
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
