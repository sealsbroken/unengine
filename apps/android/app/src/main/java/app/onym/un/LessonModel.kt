package app.onym.un

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.Surface
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import kotlinx.coroutines.flow.MutableStateFlow
import uniffi.un_ffi.*
import java.io.File
import java.util.concurrent.Executors

/**
 * The app's state, as the iOS `LessonModel`: the children and their progress, the lessons the
 * library offers, and the running lesson: CameraX frames into the shared perception, the lesson
 * in the shared engine, speech through Android's APIs. The language, the child and the last
 * lesson are remembered between runs.
 */
class LessonModel(private val activity: ComponentActivity) {
    enum class EndPhase { CELEBRATE, ASK, BYE }
    /** The tutor's state for the panel and the orb. */
    enum class TutorState { SPEAKING, LISTENING, WATCHING, CELEBRATING, MUTED }

    private val context: Context get() = activity
    private val main = Handler(Looper.getMainLooper())
    val snapshot = MutableStateFlow<UnSnapshot?>(null)
    val running = MutableStateFlow(false)
    /** After a completed lesson: the stamped ticket (3 s), then "Продолжим?", then the goodbye. */
    val endPhase = MutableStateFlow<EndPhase?>(null)
    val tutorState = MutableStateFlow(TutorState.WATCHING)
    /** The latest table state and the size of the frame it came from, for the overlay. */
    val table = MutableStateFlow<UnTableState?>(null)
    val frameSize = MutableStateFlow(Pair(0, 0))
    /** The camera's own picture, filled to the view keeping the frame's aspect (like iOS). */
    val previewView: PreviewView by lazy { PreviewView(context).apply { scaleType = PreviewView.ScaleType.FILL_CENTER } }
    private var session: UnSession? = null
    /** Set from a background thread once the detector is loaded; frames wait until then. */
    @Volatile private var perception: UnPerception? = null
    /** The detector is still loading (shown to the grown-up). */
    val preparingCamera = MutableStateFlow(false)
    /** The detector did not load; the lesson runs without the camera's count. */
    val detectorFailed = MutableStateFlow(false)
    private val prefs = context.getSharedPreferences("un", Context.MODE_PRIVATE)
    /** The tutor's language, remembered between runs; the device's language the first time. */
    val language = MutableStateFlow(savedLanguage())
    /** The lessons the library offers, in its order. */
    val lessons = MutableStateFlow<List<UnStrategyInfo>>(emptyList())
    /** The lesson last started. */
    val lessonId = MutableStateFlow(prefs.getString("lesson", "") ?: "")

    fun reloadLessons() { lessons.value = availableStrategies() }

    /** The children and their progress, in the app's files. */
    val profiles = UnProfiles(File(context.filesDir, "profiles.json").path)
    val children = MutableStateFlow<List<UnProfile>>(emptyList())
    /** The child the lessons are for, remembered between runs; null shows the children. */
    val childId = MutableStateFlow(prefs.getString("child", null))
    /** The running lesson's end has been recorded for the child. */
    private var recorded = false
    /** When the running lesson was completed (the end screens follow it). */
    private var endedAt: Long? = null
    /** "No" to "Продолжим?" tapped (the goodbye screen). */
    private var declined = false
    /** When the grown-up (or the child, saying "пауза") asked to stop. */
    private var leavingSince: Long? = null

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

    /**
     * Loads the detector in the background, from the lesson list on: by the time a lesson starts
     * the camera already counts the toys that are out.
     */
    fun preparePerception() {
        if (perception != null || preparingCamera.value) return
        preparingCamera.value = true
        detectorFailed.value = false
        Thread {
            val loaded = runCatching {
                UnPerception(UnPerceptionConfig(ModelAssets.unpack(context), "toys", 0.25f, null, null))
            }.onFailure { android.util.Log.e("Un", "the toy detector failed to load", it) }.getOrNull()
            perception = loaded
            main.post {
                preparingCamera.value = false
                detectorFailed.value = loaded == null
            }
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
        if (running.value) restart()
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
        // The child's name (the tutor greets them and does not ask it) and what their lessons
        // remembered: each child goes on where they stopped.
        if (child != null) config = profiles.sessionConfig(child, config)
        recorded = false
        endedAt = null
        declined = false
        leavingSince = null
        endPhase.value = null
        session = try {
            // The microphone stays open for the whole lesson, so the child can answer at any time.
            UnSession(lesson.id, config, AndroidSpeechInput(context), AndroidSpeechOutput(context), true, true)
        } catch (e: Exception) {
            android.util.Log.e("Un", "start failed", e)
            return
        }
        preparePerception()
        // One camera for the whole visit: the next lesson keeps it.
        startCamera()
        // After a completed lesson the tutor asks "Продолжим?"; a yes starts the next one.
        session?.offerNextLesson(nextLessonId() != null)
        session?.start()
        running.value = true
        // The phone lies on the table for minutes with nobody touching it: no screen timeout
        // while a lesson runs.
        activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        main.removeCallbacks(tick)
        main.post(tick)
    }

    /** Ten times a second: the snapshot, the lesson's end recorded, a pause or the next lesson. */
    private val tick: Runnable = object : Runnable {
        override fun run() {
            val s = session?.snapshot() ?: return
            snapshot.value = s
            val now = SystemClock.elapsedRealtime()
            if (s.summary != null && endedAt == null) endedAt = now
            // The lesson's end, recorded for the child once: progress, what it remembered.
            val child = childId.value
            val summary = s.summary
            if (summary != null && !recorded && child != null) {
                recorded = true
                profiles.record(child, lessonId.value, summary)
                reloadChildren()
            }
            // Paused (the button, or "пауза" said aloud): back to the trail once the goodbye has
            // been said and the lesson recorded, at the latest after a few seconds.
            if (s.pauseRequested || leavingSince != null) {
                val since = leavingSince ?: now.also { leavingSince = it }
                val said = s.status == UnStatus.FINISHED && (recorded || child == null)
                if (said || now - since > 8_000) {
                    end()
                    return
                }
            }
            if (s.nextLessonRequested) {
                val next = child?.let { profiles.nextLesson(it) } ?: nextLessonId()
                if (next != null) {
                    close(keepCamera = true)
                    start(next)
                    return
                }
            }
            endPhase.value = endPhase(s, now)
            tutorState.value = tutorState(s)
            main.postDelayed(this, 100)
        }
    }

    private fun endPhase(s: UnSnapshot, now: Long): EndPhase? {
        val summary = s.summary ?: return null
        val ended = endedAt ?: return null
        if (summary.paused || leavingSince != null || s.pauseRequested) return null
        if (declined || s.nextLessonDeclined) return EndPhase.BYE
        if (now - ended < 3_500) return EndPhase.CELEBRATE
        return if (nextLessonId() != null) EndPhase.ASK else EndPhase.BYE
    }

    private fun tutorState(s: UnSnapshot): TutorState {
        s.celebratedSecondsAgo?.let { if (it < 3) return TutorState.CELEBRATING }
        if (s.showNumberPad) return TutorState.MUTED
        return when (s.status) {
            UnStatus.SPEAKING -> TutorState.SPEAKING
            UnStatus.LISTENING -> TutorState.LISTENING
            else -> TutorState.WATCHING
        }
    }

    /** The yes/no cards under "Продолжим?": the same as saying it. */
    fun answerNext(yes: Boolean) {
        session?.answerNextLesson(yes)
        val next = childId.value?.let { profiles.nextLesson(it) } ?: nextLessonId()
        if (yes && next != null) {
            close(keepCamera = true)
            start(next)
        } else if (!yes) {
            declined = true
            snapshot.value?.let { endPhase.value = endPhase(it, SystemClock.elapsedRealtime()) }
        }
    }

    /** "Back to children" after the goodbye. */
    fun goHome() {
        end()
        chooseChild(null)
    }

    /** The lesson after the current one in the list, if any. */
    private fun nextLessonId(): String? {
        val list = lessons.value
        val i = list.indexOfFirst { it.id == lessonId.value }
        return if (i >= 0 && i + 1 < list.size) list[i + 1].id else null
    }

    private var provider: ProcessCameraProvider? = null
    private var analysis: ImageAnalysis? = null
    private val analysisExecutor = Executors.newSingleThreadExecutor()
    private val started = System.nanoTime()

    private fun startCamera() {
        if (provider != null) return
        val future = ProcessCameraProvider.getInstance(context)
        future.addListener({
            val provider = future.get().also { this.provider = it }
            val rotation = activity.display?.rotation ?: Surface.ROTATION_0
            val preview = Preview.Builder().build().also { it.surfaceProvider = previewView.surfaceProvider }
            // Frames come out upright (as the preview shows them), so the overlay maps 1:1.
            val analysis = ImageAnalysis.Builder()
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .setOutputImageRotationEnabled(true)
                .setTargetRotation(rotation)
                .build()
                .also { this.analysis = it }
            // Off the main thread: the detector takes tens of milliseconds a frame.
            analysis.setAnalyzer(analysisExecutor) { image -> analyze(image) }
            provider.unbindAll()
            provider.bindToLifecycle(activity, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
        }, ContextCompat.getMainExecutor(context))
    }

    /** The screen turned (a tablet): frames follow, so the boxes stay on the toys. */
    fun displayRotated() {
        activity.display?.rotation?.let { analysis?.targetRotation = it }
    }

    private fun analyze(image: ImageProxy) {
        image.use {
            val y = it.planes[0]; val uv = it.planes[1]   // YUV_420_888; plane 1 interleaved on most devices (NV12-like)
            val yBytes = ByteArray(y.buffer.remaining()).also { b -> y.buffer.get(b) }
            val uvBytes = ByteArray(uv.buffer.remaining() + 1).also { b -> uv.buffer.get(b, 0, uv.buffer.remaining()) }
            val t = (System.nanoTime() - started) / 1e9
            val state = runCatching {
                perception?.processNv12(yBytes, uvBytes, it.width.toUInt(), it.height.toUInt(), y.rowStride.toUInt(), uv.rowStride.toUInt(), t)
            }.getOrNull() ?: return
            session?.observe(state.count, state.isStable, state.groups)
            table.value = state
            frameSize.value = Pair(it.width, it.height)
        }
    }

    fun restart() { close(keepCamera = true); start() }

    /**
     * Stops the lesson as paused (the child goes on with it next time) and goes back to the trail
     * after the tutor's goodbye, as "пауза" said aloud does. A second press leaves at once.
     */
    fun pause() {
        if (leavingSince != null) return end()
        leavingSince = SystemClock.elapsedRealtime()
        session?.stop()
    }

    /** Ends the lesson at once and goes back to the trail. */
    fun end() {
        leavingSince = null
        close()
        running.value = false
        snapshot.value = null
        endPhase.value = null
    }

    fun tap(n: UInt) { session?.tap(n) }
    fun repeat() { session?.repeat() }
    fun skip() { session?.skip() }

    /** Ends the session; the camera too unless another lesson follows at once. */
    fun close(keepCamera: Boolean = false) {
        main.removeCallbacks(tick)
        activity.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        if (!keepCamera) {
            provider?.unbindAll()
            provider = null
            analysis = null
            table.value = null
        }
        // Torn down off the main thread: shutting down waits for the engine's threads, which may
        // be finishing a line.
        val closing = session
        session = null
        Thread { closing?.shutdown() }.start()
    }
}

/** The toy detector, copied out of the assets once per installed version (ONNX Runtime reads files). */
object ModelAssets {
    fun unpack(context: Context): String {
        val target = File(context.filesDir, "models")
        val stamp = File(target, ".version")
        val version = "${BuildConfig.VERSION_CODE}-${context.packageManager.getPackageInfo(context.packageName, 0).lastUpdateTime}"
        if (!stamp.exists() || stamp.readText() != version) {
            target.deleteRecursively()
            target.mkdirs()
            context.assets.list("models")?.forEach { name ->
                context.assets.open("models/$name").use { input -> File(target, name).outputStream().use { input.copyTo(it) } }
            }
            stamp.writeText(version)
        }
        return target.absolutePath
    }
}
