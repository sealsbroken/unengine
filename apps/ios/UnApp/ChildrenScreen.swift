import SwiftUI

/// The start screen, for the grown-up: one card per child (initial in the child's colour, a
/// ten-step progress bar, the next lesson and a direct Continue), "Add a child" (a name only),
/// Remove behind "…" with a confirmation.
struct ChildrenScreen: View {
    @ObservedObject var model: LessonModel
    @State private var adding = false
    @State private var removing: UnProfile?
    @Environment(\.horizontalSizeClass) private var sizeClass

    var body: some View {
        let t = Copy(ru: model.language == .russian)
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("Un").font(Un.rounded(30)).foregroundStyle(Un.orange).accessibilityAddTraits(.isHeader)
                    Spacer()
                    LanguageSwitch(language: $model.language)
                }
                .padding(.horizontal, 20)
                Text(t("children.title")).font(.system(size: 34, weight: .bold)).tracking(-0.75)
                    .padding(.horizontal, 20).accessibilityAddTraits(.isHeader)
                CameraStatusCard(model: model).padding(.horizontal, 16)
                if model.children.isEmpty {
                    empty(t)
                } else {
                    LazyVGrid(columns: columns, spacing: 16) {
                        ForEach(Array(model.children.enumerated()), id: \.element.id) { index, child in
                            ChildCard(model: model, child: child, accent: Un.accents[index % Un.accents.count],
                                      onRemove: { removing = child })
                        }
                        AddChildTile(title: t("children.add")) { adding = true }
                    }
                    .padding(.horizontal, 16)
                }
            }
            .padding(.vertical, 12)
        }
        .background(Un.surface.ignoresSafeArea())
        .sheet(isPresented: $adding) { AddChildSheet(model: model) }
        .alert(t("remove.title", ["name": removing?.name ?? ""]), isPresented: Binding(
            get: { removing != nil }, set: { if !$0 { removing = nil } })) {
            Button(t("remove.confirm"), role: .destructive) {
                if let id = removing?.id { model.removeChild(id) }
                removing = nil
            }
            Button(t("remove.cancel"), role: .cancel) { removing = nil }
        } message: {
            Text(t("remove.body"))
        }
    }

    private var columns: [GridItem] {
        sizeClass == .regular
            ? [GridItem(.flexible(), spacing: 16, alignment: .top), GridItem(.flexible(), spacing: 16, alignment: .top)]
            : [GridItem(.flexible(), alignment: .top)]
    }

    private func empty(_ t: Copy) -> some View {
        VStack(spacing: 14) {
            Image(systemName: "person.crop.circle.badge.plus").font(.system(size: 52)).foregroundStyle(Un.orange)
            Text(t("children.emptyTitle")).font(Un.rounded(30)).multilineTextAlignment(.center)
            Text(t("children.emptyBody")).font(.system(size: 17)).foregroundStyle(Un.text2)
                .multilineTextAlignment(.center).frame(maxWidth: 420)
            PrimaryButton(title: t("children.add"), icon: "plus") { adding = true }.frame(maxWidth: 360)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 48)
        .padding(.horizontal, 16)
    }
}

/// One child: initial, name, progress, the next lesson and Continue. The header opens the trail.
struct ChildCard: View {
    @ObservedObject var model: LessonModel
    let child: UnProfile
    let accent: Color
    let onRemove: () -> Void

    var body: some View {
        let t = Copy(ru: model.language == .russian)
        let ru = model.language == .russian
        let done = model.lessons.filter { child.completed.contains($0.id) }.count
        let next = model.nextLesson(for: child)
        let paused = next.map { child.paused == $0.id } ?? false
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                Button { model.childId = child.id } label: {
                    HStack(spacing: 12) {
                        Text(String(child.name.prefix(1)).uppercased()).font(Un.rounded(22))
                            .foregroundStyle(accent).frame(width: 44, height: 44)
                            .background(accent.opacity(0.16), in: Circle())
                        VStack(alignment: .leading, spacing: 2) {
                            Text(child.name).font(.system(size: 22, weight: .semibold)).foregroundStyle(Un.text)
                            Text(t("children.progress", ["n": "\(done)", "total": "\(model.lessons.count)"]))
                                .font(.system(size: 15)).foregroundStyle(Un.text2)
                        }
                        Spacer(minLength: 0)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(child.name), \(t("children.progress", ["n": "\(done)", "total": "\(model.lessons.count)"]))")
                .accessibilityHint(next.map { t("children.next", ["lesson": LessonMeta.of($0.id, in: model.lessons, ru: ru).title]) } ?? "")
                Menu {
                    Button(role: .destructive, action: onRemove) { Label(t("remove.confirm"), systemImage: "trash") }
                } label: {
                    Image(systemName: "ellipsis").font(.system(size: 16, weight: .semibold)).foregroundStyle(Un.text2)
                        .frame(width: 36, height: 36).background(Un.surface3, in: Circle())
                }
                .accessibilityLabel(t("children.more", ["name": child.name]))
            }
            ProgressSegments(model: model, child: child)
            if let next {
                let meta = LessonMeta.of(next.id, in: model.lessons, ru: ru)
                HStack(spacing: 8) {
                    Text("\(meta.number)").font(Un.rounded(12)).foregroundStyle(paused ? .white : accent)
                        .frame(width: 22, height: 22).background(paused ? Un.orange : accent.opacity(0.16), in: Circle())
                    Text(paused ? t("children.resume", ["lesson": meta.title]) : t("children.next", ["lesson": meta.title]))
                        .font(.system(size: 15, weight: .medium)).foregroundStyle(Un.text)
                }
                PrimaryButton(title: t("children.cont")) {
                    model.childId = child.id
                    model.start(next.id)
                }
            }
        }
        .padding(16)
        .background(Un.surface2, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
}

/// Ten segments: green done, orange paused, the rest quiet.
struct ProgressSegments: View {
    @ObservedObject var model: LessonModel
    let child: UnProfile

    var body: some View {
        HStack(spacing: 4) {
            ForEach(model.lessons, id: \.id) { lesson in
                Capsule()
                    .fill(child.completed.contains(lesson.id) ? Un.green : child.paused == lesson.id ? Un.orange : Un.surface3)
                    .frame(height: 6)
            }
        }
        .accessibilityHidden(true)
    }
}

/// The dashed "Add a child" tile at the end of the grid.
struct AddChildTile: View {
    let title: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 10) {
                Image(systemName: "plus").font(.system(size: 22, weight: .semibold)).foregroundStyle(Un.blue)
                    .frame(width: 48, height: 48).background(Un.blue.opacity(0.14), in: Circle())
                Text(title).font(.system(size: 16, weight: .semibold)).foregroundStyle(Un.blue)
            }
            .frame(maxWidth: .infinity, minHeight: 180)
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(Un.text3, style: StrokeStyle(lineWidth: 1.5, dash: [6, 5])))
        }
        .buttonStyle(.plain)
    }
}

/// "New child": a name, nothing else (the colour is given automatically).
struct AddChildSheet: View {
    @ObservedObject var model: LessonModel
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @FocusState private var focused: Bool

    var body: some View {
        let t = Copy(ru: model.language == .russian)
        NavigationStack {
            VStack(alignment: .leading, spacing: 8) {
                TextField(t("add.placeholder"), text: $name)
                    .font(.system(size: 17))
                    .padding(14)
                    .background(Un.surface2, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).stroke(focused ? Un.blue : Un.hairline))
                    .focused($focused)
                    .submitLabel(.done)
                    .onSubmit(add)
                Text(t("add.helper")).font(.system(size: 12.5)).foregroundStyle(Un.text2).padding(.horizontal, 4)
                Spacer()
            }
            .padding(16)
            .background(Un.surface.ignoresSafeArea())
            .navigationTitle(t("add.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(t("add.cancel")) { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(t("add.confirm"), action: add).disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .onAppear { focused = true }
        }
        .presentationDetents([.medium])
    }

    private func add() {
        guard !name.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        model.addChild(name)
        model.childId = nil
        dismiss()
    }
}
