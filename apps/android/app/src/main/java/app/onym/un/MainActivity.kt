package app.onym.un

import android.Manifest
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
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
        model.reloadChildren()
        model.preparePerception()
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
    val childId by model.childId.collectAsState()
    when {
        running -> LessonScreen(model)
        childId == null -> ChildrenList(model)
        else -> LessonList(model)
    }
}

/** As on iOS: the children, each with their progress; a tap opens that child's lessons. */
@Composable
fun ChildrenList(model: LessonModel) {
    val children by model.children.collectAsState()
    val lessons by model.lessons.collectAsState()
    val ru = model.language.collectAsState().value == UnLanguage.RUSSIAN
    var adding by remember { mutableStateOf(false) }
    var name by remember { mutableStateOf("") }
    var removing by remember { mutableStateOf<UnProfile?>(null) }
    Column(Modifier.fillMaxSize().padding(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(if (ru) "Дети" else "Children", style = MaterialTheme.typography.headlineMedium, modifier = Modifier.weight(1f))
            LanguageChips(model, model.language.collectAsState().value)
            TextButton(onClick = { adding = true }) { Text(if (ru) "+ Добавить" else "+ Add") }
        }
        if (children.isEmpty()) {
            Text(if (ru) "Добавьте ребёнка: у каждого свой прогресс по урокам." else "Add a child: each goes through the lessons at their own pace.",
                Modifier.padding(top = 24.dp))
        }
        LazyColumn(Modifier.fillMaxWidth().weight(1f).padding(top = 12.dp)) {
            items(children, key = { it.id }) { child ->
                val done = lessons.count { child.completed.contains(it.id) }
                val next = model.nextLesson(child)?.title
                ListItem(
                    headlineContent = { Text(child.name, style = MaterialTheme.typography.titleLarge) },
                    supportingContent = {
                        Text((if (ru) "Пройдено уроков: $done из ${lessons.size}" else "$done of ${lessons.size} lessons done") +
                            (next?.let { if (ru) " · дальше: $it" else " · next: $it" } ?: ""))
                    },
                    trailingContent = { TextButton(onClick = { removing = child }) { Text(if (ru) "Удалить" else "Remove") } },
                    modifier = Modifier.clickable { model.chooseChild(child.id) },
                )
                HorizontalDivider()
            }
        }
        CameraStatus(model, Modifier.fillMaxWidth())
    }
    if (adding) {
        AlertDialog(
            onDismissRequest = { adding = false; name = "" },
            title = { Text(if (ru) "Новый ребёнок" else "New child") },
            text = { OutlinedTextField(name, { name = it }, label = { Text(if (ru) "Имя" else "Name") }, singleLine = true) },
            confirmButton = { TextButton(onClick = { model.addChild(name); name = ""; adding = false }) { Text(if (ru) "Добавить" else "Add") } },
            dismissButton = { TextButton(onClick = { adding = false; name = "" }) { Text(if (ru) "Отмена" else "Cancel") } },
        )
    }
    removing?.let { child ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(if (ru) "Удалить ${child.name}?" else "Remove ${child.name}?") },
            text = { Text(if (ru) "Прогресс ребёнка будет удалён." else "The child's progress is deleted.") },
            confirmButton = { TextButton(onClick = { model.removeChild(child.id); removing = null }) { Text(if (ru) "Удалить" else "Remove") } },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(if (ru) "Отмена" else "Cancel") } },
        )
    }
}

/** As on iOS: the chosen child's lessons; completed ones are ticked, the next is marked; a tap starts one. */
@Composable
fun LessonList(model: LessonModel) {
    val lessons by model.lessons.collectAsState()
    val language by model.language.collectAsState()
    val children by model.children.collectAsState()
    val childId by model.childId.collectAsState()
    val child = children.firstOrNull { it.id == childId }
    val next = child?.let { model.nextLesson(it) }
    val ru = language == UnLanguage.RUSSIAN
    Column(Modifier.fillMaxSize().padding(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = { model.chooseChild(null) }) { Text(if (ru) "‹ Дети" else "‹ Children") }
            Text(child?.name ?: (if (ru) "Уроки" else "Lessons"), style = MaterialTheme.typography.headlineMedium,
                modifier = Modifier.weight(1f))
            LanguageChips(model, language)
        }
        if (next != null) {
            Button(onClick = { model.start(next.id) }, Modifier.fillMaxWidth().padding(top = 8.dp)) {
                Text((if (ru) "Продолжить: " else "Continue: ") + next.title)
            }
        }
        if (lessons.isEmpty()) {
            Text(if (language == UnLanguage.RUSSIAN) "Уроков нет" else "No lessons", Modifier.padding(top = 24.dp))
        }
        LazyColumn(Modifier.fillMaxWidth().weight(1f).padding(top = 12.dp)) {
            items(lessons, key = { it.id }) { lesson ->
                ListItem(
                    headlineContent = { Text(lesson.title) },
                    supportingContent = { Text(lesson.description) },
                    trailingContent = {
                        when {
                            child?.completed?.contains(lesson.id) == true -> Text("✓", style = MaterialTheme.typography.titleLarge)
                            lesson.id == next?.id -> Text("→", style = MaterialTheme.typography.titleLarge)
                        }
                    },
                    modifier = Modifier.clickable { model.start(lesson.id) },
                )
                HorizontalDivider()
            }
        }
        CameraStatus(model, Modifier.fillMaxWidth())
    }
}

/**
 * As on iOS: while the detector loads (or when it failed), a card the grown-up can read: the
 * camera does not count yet.
 */
@Composable
fun CameraStatus(model: LessonModel, modifier: Modifier = Modifier) {
    val preparing by model.preparingCamera.collectAsState()
    val failed by model.detectorFailed.collectAsState()
    val ru = model.language.collectAsState().value == UnLanguage.RUSSIAN
    if (!preparing && !failed) return
    Card(modifier.padding(16.dp).widthIn(max = 420.dp)) {
        Column(Modifier.padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            if (preparing) {
                CircularProgressIndicator()
                Text(if (ru) "Готовим камеру…" else "Preparing the camera…", style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.padding(top = 8.dp))
                Text(if (ru) "После установки или обновления это занимает до минуты. Урок можно начинать: игрушки будут видны, как только камера будет готова."
                     else "After an install or update this takes up to a minute. You can start a lesson: the toys are seen as soon as the camera is ready.",
                    style = MaterialTheme.typography.bodySmall, textAlign = TextAlign.Center)
            } else {
                Text(if (ru) "Камера не смогла загрузить распознавание игрушек." else "The camera could not load toy recognition.",
                    color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
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
    // Back (the button top left, or the system back): pauses the lesson and returns to the list, as on iOS.
    BackHandler { model.pause() }
    Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Row(Modifier.fillMaxWidth()) {
            TextButton(onClick = { model.pause() }) {
                Text(if (language == UnLanguage.RUSSIAN) "‹ К приключениям" else "‹ Back to adventures")
            }
        }
        Text(snapshot?.lastSpoken ?: "…", style = MaterialTheme.typography.headlineSmall, textAlign = TextAlign.Center)
        Text(snapshot?.let { "$title  ${it.status}  level ${it.level + 1u}/${it.levelCount}  ${it.correct}✓/${it.tasksCompleted}" } ?: title,
            style = MaterialTheme.typography.bodySmall)
        Box(Modifier.weight(1f).fillMaxWidth().padding(vertical = 12.dp).clip(RoundedCornerShape(16.dp))) {
            AndroidView(factory = { model.previewView }, modifier = Modifier.fillMaxSize())
            TableOverlay(table, frameSize, snapshot?.highlightCount, Modifier.fillMaxSize())
            CameraStatus(model, Modifier.align(Alignment.Center))
        }
        if (snapshot?.showNumberPad == true) {
            LazyVerticalGrid(GridCells.Fixed(5), Modifier.fillMaxWidth()) {
                items((0..10).toList()) { n ->
                    Button(onClick = { model.tap(n.toUInt()) }, Modifier.padding(2.dp)) { Text("$n", style = MaterialTheme.typography.titleLarge) }
                }
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Button(onClick = { model.pause() }) { Text(if (language == UnLanguage.RUSSIAN) "Пауза" else "Pause") }
            TextButton(onClick = { model.repeat() }) { Text("Repeat") }
            TextButton(onClick = { model.skip() }) { Text("Skip") }
            LanguageChips(model, language)
        }
    }
}
