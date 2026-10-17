package app.onym.un

import android.Manifest
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
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
 * As on iOS: the list of lessons the library offers, then the lesson: the tutor's line, status,
 * the camera's picture with the overlay, the number pad, end, repeat, skip and the language.
 * Perception and the lesson run in Rust.
 */
class MainActivity : ComponentActivity() {
    private val model by lazy { LessonModel(this) }

    private val permissions = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // The phone lies on the table for minutes with nobody touching it: no screen timeout.
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        permissions.launch(arrayOf(Manifest.permission.CAMERA, Manifest.permission.RECORD_AUDIO))
        model.reloadLessons()
        setContent { MaterialTheme { Screen(model) } }
    }

    override fun onDestroy() {
        model.close()
        super.onDestroy()
    }
}

@Composable
fun Screen(model: LessonModel) {
    val running by model.running.collectAsState()
    if (running) LessonScreen(model) else LessonList(model)
}

/** Every lesson the library offers, in its order; a tap starts one. The last lesson played is ticked. */
@Composable
fun LessonList(model: LessonModel) {
    val lessons by model.lessons.collectAsState()
    val lessonId by model.lessonId.collectAsState()
    val language by model.language.collectAsState()
    Column(Modifier.fillMaxSize().padding(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(if (language == UnLanguage.RUSSIAN) "Уроки" else "Lessons", style = MaterialTheme.typography.headlineMedium,
                modifier = Modifier.weight(1f))
            LanguageChips(model, language)
        }
        if (lessons.isEmpty()) {
            Text(if (language == UnLanguage.RUSSIAN) "Уроков нет" else "No lessons", Modifier.padding(top = 24.dp))
        }
        LazyColumn(Modifier.fillMaxWidth().padding(top = 12.dp)) {
            items(lessons, key = { it.id }) { lesson ->
                ListItem(
                    headlineContent = { Text(lesson.title) },
                    supportingContent = { Text(lesson.description) },
                    trailingContent = { if (lesson.id == lessonId) Text("✓", style = MaterialTheme.typography.titleLarge) },
                    modifier = Modifier.clickable { model.start(lesson.id) },
                )
                HorizontalDivider()
            }
        }
    }
}

/** The tutor's language; a running lesson starts over in the new one. */
@Composable
fun LanguageChips(model: LessonModel, language: UnLanguage) {
    FilterChip(selected = language == UnLanguage.ENGLISH, onClick = { model.setLanguage(UnLanguage.ENGLISH) },
        label = { Text("EN") }, modifier = Modifier.padding(2.dp))
    FilterChip(selected = language == UnLanguage.RUSSIAN, onClick = { model.setLanguage(UnLanguage.RUSSIAN) },
        label = { Text("RU") }, modifier = Modifier.padding(2.dp))
}

@Composable
fun LessonScreen(model: LessonModel) {
    val snapshot by model.snapshot.collectAsState()
    val language by model.language.collectAsState()
    val lessons by model.lessons.collectAsState()
    val lessonId by model.lessonId.collectAsState()
    val title = lessons.firstOrNull { it.id == lessonId }?.title ?: lessonId
    val table by model.table.collectAsState()
    val frameSize by model.frameSize.collectAsState()
    Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(snapshot?.lastSpoken ?: "…", style = MaterialTheme.typography.headlineSmall, textAlign = TextAlign.Center)
        Text(snapshot?.let { "$title  ${it.status}  level ${it.level + 1u}/${it.levelCount}  ${it.correct}✓/${it.tasksCompleted}" } ?: title,
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
            Button(onClick = { model.end() }) { Text("End lesson") }
            TextButton(onClick = { model.repeat() }) { Text("Repeat") }
            TextButton(onClick = { model.skip() }) { Text("Skip") }
            LanguageChips(model, language)
        }
    }
}
