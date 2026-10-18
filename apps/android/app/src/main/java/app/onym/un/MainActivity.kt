package app.onym.un

import android.Manifest
import android.content.res.Configuration
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.tween
import androidx.compose.runtime.*
import com.sun.jna.Library
import com.sun.jna.Native

/**
 * The app, as on iOS: the children, a child's adventures (the trail), and the lesson over them.
 * Perception and the lesson run in the shared Rust library (the engine with the unmath lessons);
 * speech through Android's APIs. The look is the "Un Design Board" (Claude Design, Onym tokens).
 */
class MainActivity : ComponentActivity() {
    private val model by lazy { LessonModel(this) }

    private val permissions = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        // The unmath lessons become the library's lesson list before anything asks for it.
        Native.load("un_mobile", MobileLibrary::class.java).un_mobile_init()
        // Camera and microphone at launch, so the lesson can start right away and is not
        // interrupted by prompts.
        permissions.launch(arrayOf(Manifest.permission.CAMERA, Manifest.permission.RECORD_AUDIO))
        model.reloadLessons()
        model.reloadChildren()
        model.preparePerception()
        setContent { UnTheme { Screen(model) } }
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        model.displayRotated()
    }

    override fun onDestroy() {
        model.close()
        super.onDestroy()
    }
}

/** `un_mobile_init`, the one C function of the library outside the UniFFI bindings. */
interface MobileLibrary : Library {
    fun un_mobile_init()
}

@Composable
fun Screen(model: LessonModel) {
    val running by model.running.collectAsState()
    val childId by model.childId.collectAsState()
    val screen = when {
        running -> 2
        childId == null -> 0
        else -> 1
    }
    Crossfade(screen, animationSpec = tween(250), label = "screen") {
        when (it) {
            2 -> LessonView(model)
            0 -> ChildrenScreen(model)
            else -> TrailScreen(model)
        }
    }
}
