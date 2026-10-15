package app.onym.un

import android.Manifest
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import uniffi.un_ffi.*

/**
 * One screen, as on iOS: the tutor's line, status, the camera's picture with the overlay, the
 * number pad, start/stop, repeat, skip and the language. Perception and the lesson run in Rust.
 * The lesson starts by itself once the permissions are answered.
 */
class MainActivity : ComponentActivity() {
    private val model by lazy { LessonModel(this) }

    private val permissions = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { granted ->
        if (granted[Manifest.permission.CAMERA] == true && !model.running.value) model.start()
    }

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
    val running by model.running.collectAsState()
    val language by model.language.collectAsState()
    val table by model.table.collectAsState()
    val frameSize by model.frameSize.collectAsState()
    Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(snapshot?.lastSpoken ?: "…", style = MaterialTheme.typography.headlineSmall, textAlign = TextAlign.Center)
        Text(snapshot?.let { "${it.status}  level ${it.level + 1u}/${it.levelCount}  ${it.correct}✓/${it.tasksCompleted}" } ?: "idle",
            style = MaterialTheme.typography.bodySmall)
        Box(Modifier.weight(1f).fillMaxWidth().padding(vertical = 12.dp).clip(RoundedCornerShape(16.dp))) {
            AndroidView(factory = { model.previewView }, modifier = Modifier.fillMaxSize())
            TableOverlay(table, frameSize, snapshot?.highlightCount, Modifier.fillMaxSize())
        }
        if (snapshot?.showNumberPad == true) {
            LazyVerticalGrid(GridCells.Fixed(5), Modifier.fillMaxWidth()) {
                items((0..10).toList()) { n ->
                    Button(onClick = { model.tap(n.toUInt()) }, Modifier.padding(2.dp)) { Text("$n", style = MaterialTheme.typography.titleLarge) }
                }
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Button(onClick = { if (running) model.stop() else model.start() }) {
                Text(if (running) "End lesson" else "Start lesson")
            }
            TextButton(onClick = { model.repeat() }) { Text("Repeat") }
            TextButton(onClick = { model.skip() }) { Text("Skip") }
            // The tutor's language; a running lesson starts over in the new one.
            FilterChip(selected = language == UnLanguage.ENGLISH, onClick = { model.setLanguage(UnLanguage.ENGLISH) },
                label = { Text("EN") }, modifier = Modifier.padding(2.dp))
            FilterChip(selected = language == UnLanguage.RUSSIAN, onClick = { model.setLanguage(UnLanguage.RUSSIAN) },
                label = { Text("RU") }, modifier = Modifier.padding(2.dp))
        }
    }
}
