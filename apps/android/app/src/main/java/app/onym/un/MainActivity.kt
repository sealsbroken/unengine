package app.onym.un

import android.Manifest
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import uniffi.un_ffi.*

/** One screen: the tutor's line, status, a number pad, start/stop. Perception and the lesson run in Rust. */
class MainActivity : ComponentActivity() {
    private val model by lazy { LessonModel(this) }

    private val permissions = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        permissions.launch(arrayOf(Manifest.permission.CAMERA, Manifest.permission.RECORD_AUDIO))
        setContent { MaterialTheme { LessonScreen(model) } }
    }

    override fun onDestroy() {
        model.close()
        super.onDestroy()
    }
}

@Composable
fun LessonScreen(model: LessonModel) {
    val snapshot by model.snapshot.collectAsState()
    Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(snapshot?.lastSpoken ?: "…", style = MaterialTheme.typography.headlineSmall)
        Text(snapshot?.let { "${it.status}  level ${it.level + 1}/${it.levelCount}  ${it.correct}✓/${it.tasksCompleted}" } ?: "idle",
            style = MaterialTheme.typography.bodySmall)
        Spacer(Modifier.weight(1f))
        if (snapshot?.showNumberPad == true) {
            Row { for (n in 0..5) Button(onClick = { model.tap(n.toUInt()) }, Modifier.padding(2.dp)) { Text("$n") } }
            Row { for (n in 6..10) Button(onClick = { model.tap(n.toUInt()) }, Modifier.padding(2.dp)) { Text("$n") } }
        }
        Row {
            Button(onClick = { if (model.running.value) model.stop() else model.start() }) {
                Text(if (model.running.value) "End lesson" else "Start lesson")
            }
            TextButton(onClick = { model.repeat() }) { Text("Repeat") }
            TextButton(onClick = { model.skip() }) { Text("Skip") }
        }
    }
}
