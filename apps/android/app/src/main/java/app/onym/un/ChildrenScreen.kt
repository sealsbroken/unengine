package app.onym.un

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.MoreHoriz
import androidx.compose.material.icons.rounded.PersonAddAlt1
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.un_ffi.*

/**
 * The start screen, for the grown-up (as on iOS): one card per child (initial in the child's
 * colour, a ten-step progress bar, the next lesson and a direct Continue), "Add a child" (a name
 * only), Remove behind "…" with a confirmation.
 */
@Composable
fun ChildrenScreen(model: LessonModel) {
    val c = Un.c
    val children by model.children.collectAsState()
    val language by model.language.collectAsState()
    val t = Copy(language == UnLanguage.RUSSIAN)
    var adding by remember { mutableStateOf(false) }
    var removing by remember { mutableStateOf<UnProfile?>(null) }
    BoxWithConstraints(Modifier.fillMaxSize().background(c.surface)) {
        val columns = if (maxWidth >= 600.dp) 2 else 1
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).windowInsetsPadding(WindowInsets.safeDrawing)
                .padding(vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(Modifier.padding(horizontal = 20.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Un", style = Un.rounded(30.sp), color = c.orange, modifier = Modifier.semantics { heading() })
                Spacer(Modifier.weight(1f))
                LanguageSwitch(language) { model.setLanguage(it) }
            }
            Text(t("children.title"), fontSize = 34.sp, fontWeight = FontWeight.Bold, letterSpacing = (-0.75).sp, color = c.text,
                modifier = Modifier.padding(horizontal = 20.dp).semantics { heading() })
            CameraStatusCard(model, Modifier.padding(horizontal = 16.dp))
            if (children.isEmpty()) {
                Empty(t) { adding = true }
            } else {
                val tiles: List<UnProfile?> = children + listOf(null)
                Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    for (row in tiles.withIndex().chunked(columns)) {
                        Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.Top) {
                            for ((index, child) in row) {
                                Box(Modifier.weight(1f)) {
                                    if (child == null) AddChildTile(t("children.add")) { adding = true }
                                    else ChildCard(model, child, c.accents[index % c.accents.size]) { removing = child }
                                }
                            }
                            repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                        }
                    }
                }
            }
        }
    }
    if (adding) AddChildSheet(model, t) { adding = false }
    removing?.let { child ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(t("remove.title", "name" to child.name)) },
            text = { Text(t("remove.body")) },
            confirmButton = {
                TextButton(onClick = { model.removeChild(child.id); removing = null }) { Text(t("remove.confirm"), color = c.red) }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(t("remove.cancel")) } },
        )
    }
}

@Composable
private fun Empty(t: Copy, onAdd: () -> Unit) {
    val c = Un.c
    Column(
        Modifier.fillMaxWidth().padding(vertical = 48.dp, horizontal = 16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Icon(Icons.Rounded.PersonAddAlt1, null, tint = c.orange, modifier = Modifier.size(52.dp))
        Text(t("children.emptyTitle"), style = Un.rounded(30.sp), color = c.text, textAlign = TextAlign.Center)
        Text(t("children.emptyBody"), fontSize = 17.sp, color = c.text2, textAlign = TextAlign.Center, modifier = Modifier.widthIn(max = 420.dp))
        PrimaryButton(t("children.add"), Modifier.widthIn(max = 360.dp), icon = Icons.Rounded.Add, onClick = onAdd)
    }
}

/** One child: initial, name, progress, the next lesson and Continue. The header opens the trail. */
@Composable
fun ChildCard(model: LessonModel, child: UnProfile, accent: androidx.compose.ui.graphics.Color, onRemove: () -> Unit) {
    val c = Un.c
    val lessons by model.lessons.collectAsState()
    val ru = model.language.collectAsState().value == UnLanguage.RUSSIAN
    val t = Copy(ru)
    val done = lessons.count { child.completed.contains(it.id) }
    val next = model.nextLesson(child)
    val paused = next != null && child.paused == next.id
    var menu by remember { mutableStateOf(false) }
    Column(
        Modifier.fillMaxWidth().background(c.surface2, RoundedCornerShape(14.dp)).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(
                Modifier.weight(1f).clip(RoundedCornerShape(10.dp)).clickable { model.chooseChild(child.id) }
                    .semantics { contentDescription = "${child.name}, ${t("children.progress", "n" to "$done", "total" to "${lessons.size}")}" },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Box(Modifier.size(44.dp).background(accent.copy(alpha = 0.16f), CircleShape), contentAlignment = Alignment.Center) {
                    Text(child.name.take(1).uppercase(), style = Un.rounded(22.sp), color = accent)
                }
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(child.name, fontSize = 22.sp, fontWeight = FontWeight.SemiBold, color = c.text)
                    Text(t("children.progress", "n" to "$done", "total" to "${lessons.size}"), fontSize = 15.sp, color = c.text2)
                }
            }
            Box {
                Box(
                    Modifier.size(36.dp).clip(CircleShape).background(c.surface3).clickable { menu = true }
                        .semantics { contentDescription = t("children.more", "name" to child.name) },
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.Rounded.MoreHoriz, null, tint = c.text2, modifier = Modifier.size(20.dp)) }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(
                        text = { Text(t("remove.confirm"), color = c.red) },
                        leadingIcon = { Icon(Icons.Rounded.Delete, null, tint = c.red) },
                        onClick = { menu = false; onRemove() },
                    )
                }
            }
        }
        ProgressSegments(lessons, child)
        if (next != null) {
            val meta = LessonMeta.of(next.id, lessons, ru)
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(
                    Modifier.size(22.dp).background(if (paused) c.orange else accent.copy(alpha = 0.16f), CircleShape),
                    contentAlignment = Alignment.Center,
                ) { Text("${meta.number}", style = Un.rounded(12.sp), color = if (paused) androidx.compose.ui.graphics.Color.White else accent) }
                Text(
                    if (paused) t("children.resume", "lesson" to meta.title) else t("children.next", "lesson" to meta.title),
                    fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text,
                )
            }
            PrimaryButton(t("children.cont")) {
                model.chooseChild(child.id)
                model.start(next.id)
            }
        }
    }
}

/** Ten segments: green done, orange paused, the rest quiet. */
@Composable
fun ProgressSegments(lessons: List<UnStrategyInfo>, child: UnProfile) {
    val c = Un.c
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        for (lesson in lessons) {
            val color = when {
                child.completed.contains(lesson.id) -> c.green
                child.paused == lesson.id -> c.orange
                else -> c.surface3
            }
            Box(Modifier.weight(1f).height(6.dp).background(color, CircleShape))
        }
    }
}

/** The dashed "Add a child" tile at the end of the grid. */
@Composable
fun AddChildTile(title: String, onClick: () -> Unit) {
    val c = Un.c
    Column(
        Modifier.fillMaxWidth().defaultMinSize(minHeight = 180.dp).clip(RoundedCornerShape(14.dp))
            .drawBehind {
                drawRoundRect(
                    c.text3, cornerRadius = CornerRadius(14.dp.toPx()),
                    style = Stroke(1.5.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(6.dp.toPx(), 5.dp.toPx()))),
                )
            }
            .clickable(onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp, Alignment.CenterVertically),
    ) {
        Box(Modifier.size(48.dp).background(c.blue.copy(alpha = 0.14f), CircleShape), contentAlignment = Alignment.Center) {
            Icon(Icons.Rounded.Add, null, tint = c.blue, modifier = Modifier.size(24.dp))
        }
        Text(title, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = c.blue)
    }
}

/** "New child": a name, nothing else (the colour is given automatically). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddChildSheet(model: LessonModel, t: Copy, onDismiss: () -> Unit) {
    val c = Un.c
    var name by remember { mutableStateOf("") }
    val focus = remember { FocusRequester() }
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    fun add() {
        if (name.isBlank()) return
        model.addChild(name)
        model.chooseChild(null)
        onDismiss()
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet, containerColor = c.surface) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = onDismiss) { Text(t("add.cancel"), fontSize = 17.sp) }
                Text(t("add.title"), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = c.text, textAlign = TextAlign.Center,
                    modifier = Modifier.weight(1f))
                TextButton(onClick = ::add, enabled = name.isNotBlank()) { Text(t("add.confirm"), fontSize = 17.sp, fontWeight = FontWeight.SemiBold) }
            }
            OutlinedTextField(
                name, { name = it },
                placeholder = { Text(t("add.placeholder")) },
                singleLine = true,
                shape = RoundedCornerShape(14.dp),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { add() }),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedContainerColor = c.surface2, unfocusedContainerColor = c.surface2,
                    focusedBorderColor = c.blue, unfocusedBorderColor = c.hairline,
                ),
                modifier = Modifier.fillMaxWidth().focusRequester(focus),
            )
            Text(t("add.helper"), fontSize = 12.5.sp, color = c.text2, modifier = Modifier.padding(horizontal = 4.dp))
        }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }
}
