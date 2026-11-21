//! The texture changes egui hands over, kept for every window until one paints them.

use egui::TextureId;

use crate::epaint::textures::TexturesDelta;

/// The texture changes of every viewport that no paint has given the painter yet.
///
/// egui's textures belong to its [`egui::Context`], not to a viewport: any window draws any
/// [`TextureId`], and one painter (glow's OpenGL context, wgpu's device) holds them all. Each
/// pass hands over every change made since the last pass ended, whichever viewport that pass was
/// for: an immediate viewport's pass, run inside its parent's, carries what the parent loaded
/// before it, and a texture made on another thread goes with whichever pass ends next.
///
/// Kept per viewport and given to the painter only when that viewport painted, a texture whose
/// changes came with a window that wasn't painted (a minimized or hidden main window, a paint
/// that failed, a window closed before it painted again) never reached the painter while the
/// other windows drew it: "Failed to find texture" (glow) or "Missing texture" (wgpu) at each of
/// their frames. So they wait here, for every window, and whichever paints first gives them.
///
/// New and changed textures are given before any paint. Freed ones wait for the end of a whole
/// frame, a top-level viewport's with the immediate viewports inside it: the parent paints after
/// them, with shapes made before them, which can still use a texture freed meanwhile.
#[derive(Default)]
pub(crate) struct PendingTextures {
    delta: TexturesDelta,
}

impl PendingTextures {
    /// A pass's changes, after the ones before it.
    pub fn append(&mut self, newer: TexturesDelta) {
        self.delta.append(newer);
    }

    /// For a paint inside a frame (an immediate viewport's): `paint` gets the new and changed
    /// textures, not the freed ones. What it leaves in them (glow: a texture OpenGL couldn't
    /// create now; wgpu: a window with no surface yet) waits for the next paint.
    pub fn without_frees<R>(&mut self, paint: impl FnOnce(&mut TexturesDelta) -> R) -> R {
        let mut sets = TexturesDelta {
            set: core::mem::take(&mut self.delta.set),
            free: Default::default(),
        };
        let painted = paint(&mut sets);
        // Nothing came in meanwhile (`self` was borrowed), but keep the order if it had.
        let newer = core::mem::replace(&mut self.delta.set, core::mem::take(&mut sets.set));
        #[expect(clippy::iter_over_hash_type)] // One id's changes keep their order.
        for (id, deltas) in newer {
            for delta in deltas {
                self.delta.push(id, delta);
            }
        }
        // `paint` was given no frees; any it added (none should) are frees all the same.
        self.delta.free.extend(core::mem::take(&mut sets.free));
        painted
    }

    /// For a frame's last paint (a top-level viewport's): every change, frees included.
    pub fn all(&mut self) -> &mut TexturesDelta {
        &mut self.delta
    }

    /// A frame that ended without its top-level viewport painted (minimized, hidden, a paint
    /// that failed): the textures freed so far are freed now with `free`, so a window that stays
    /// hidden while its dialogs draw doesn't keep every texture they let go. A change still
    /// waiting for one of them is dropped with it.
    pub fn free_now(&mut self, mut free: impl FnMut(TextureId)) {
        #[expect(clippy::iter_over_hash_type)] // Order doesn't matter here
        for id in self.delta.free.drain() {
            self.delta.set.remove(&id);
            free(id);
        }
    }
}

impl Drop for PendingTextures {
    fn drop(&mut self) {
        // Changes no paint will give anymore (the app is ending): not a mistake.
        self.delta.clear();
    }
}

#[cfg(test)]
mod tests {
    use core::cell::RefCell;
    use std::collections::HashSet;
    use std::rc::Rc;

    use egui::epaint::{ClippedPrimitive, Primitive};
    use egui::{
        Color32, ColorImage, Context, ImmediateViewport, Pos2, RawInput, Rect, TextureHandle,
        TextureOptions, ViewportBuilder, ViewportId, pos2, vec2,
    };

    use super::*;

    /// A painter that only tells textures apart: the ones given to it and not freed since.
    #[derive(Default)]
    struct Painter {
        textures: HashSet<TextureId>,
        /// The textures a paint drew that it didn't have.
        missing: Vec<TextureId>,
    }

    impl Painter {
        fn paint(&mut self, primitives: &[ClippedPrimitive], delta: &mut TexturesDelta) {
            #[expect(clippy::iter_over_hash_type)]
            for (id, _) in delta.set.drain() {
                self.textures.insert(id);
            }
            for p in primitives {
                if let Primitive::Mesh(mesh) = &p.primitive
                    && !self.textures.contains(&mesh.texture_id)
                {
                    self.missing.push(mesh.texture_id);
                }
            }
            #[expect(clippy::iter_over_hash_type)]
            for id in delta.free.drain() {
                self.textures.remove(&id);
            }
        }
    }

    fn input(viewport_id: ViewportId) -> RawInput {
        RawInput {
            viewport_id,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(200.0, 100.0))),
            ..Default::default()
        }
    }

    fn picture(ctx: &Context) -> TextureHandle {
        let image = ColorImage::filled([4, 4], Color32::RED);
        ctx.load_texture("picture", image, TextureOptions::LINEAR)
    }

    fn draw(ui: &egui::Ui, texture: &TextureHandle) {
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let rc = Rect::from_min_size(pos2(10.0, 10.0), vec2(16.0, 16.0));
        ui.painter().image(texture.id(), rc, uv, Color32::WHITE);
    }

    /// The main window minimized or hidden while one of its dialogs draws: its pass still runs
    /// (for the dialog), and a texture it makes after the dialog's pass ended comes with its own
    /// output, which no paint gives. The dialog draws that texture in the next frames: it must
    /// have it, as eframe's integrations do through [`PendingTextures`].
    #[test]
    fn a_texture_that_came_with_a_window_not_painted_reaches_the_window_that_draws_it() {
        let ctx = Context::default();
        ctx.set_embed_viewports(false);
        let pending = Rc::new(RefCell::new(PendingTextures::default()));
        let painter = Rc::new(RefCell::new(Painter::default()));
        {
            let ctx2 = ctx.clone();
            let pending = Rc::clone(&pending);
            let painter = Rc::clone(&painter);
            ctx.set_immediate_viewport_renderer(move |ctx, viewport: ImmediateViewport<'_>| {
                let ImmediateViewport {
                    ids,
                    mut viewport_ui_cb,
                    ..
                } = viewport;
                let out = ctx.run_ui(input(ids.this), |ui| viewport_ui_cb(ui));
                let primitives = ctx2.tessellate(out.shapes, out.pixels_per_point);
                let mut pending = pending.borrow_mut();
                pending.append(out.textures_delta);
                pending.without_frees(|delta| painter.borrow_mut().paint(&primitives, delta));
            });
        }

        let shared: Rc<RefCell<Option<TextureHandle>>> = Rc::default();
        let dialog = ViewportId::from_hash_of("dialog");
        for frame in 0..3 {
            let out = ctx.run_ui(input(ViewportId::ROOT), |ui| {
                let shared = Rc::clone(&shared);
                ui.ctx().show_viewport_immediate(
                    dialog,
                    ViewportBuilder::default().with_title("dialog"),
                    move |ui, _| {
                        if let Some(texture) = &*shared.borrow() {
                            draw(ui, texture);
                        }
                    },
                );
                // After the dialog's pass ended: this one's output carries it.
                if frame == 0 {
                    *shared.borrow_mut() = Some(picture(ui.ctx()));
                }
            });
            // The main window isn't painted: its frame ends without a paint.
            let mut pending = pending.borrow_mut();
            pending.append(out.textures_delta);
            pending.free_now(|id| {
                painter.borrow_mut().textures.remove(&id);
            });
        }

        let painter = painter.borrow();
        let id = shared.borrow().as_ref().map(TextureHandle::id);
        assert!(id.is_some_and(|id| painter.textures.contains(&id)));
        assert_eq!(
            painter.missing,
            vec![],
            "the dialog drew a texture it didn't have"
        );
    }

    /// A texture let go while its window's frame runs is freed after that whole frame, not at
    /// the paint of an immediate viewport inside it: the parent still paints with it.
    #[test]
    fn a_freed_texture_waits_for_the_end_of_the_frame() {
        let ctx = Context::default();
        let texture = picture(&ctx);
        let id = texture.id();
        let mut pending = PendingTextures::default();
        let mut painter = Painter::default();

        pending.append(ctx.tex_manager().write().take_delta());
        drop(texture);
        // An immediate viewport's paint: the texture is given, not freed.
        pending.append(ctx.tex_manager().write().take_delta());
        pending.without_frees(|delta| painter.paint(&[], delta));
        assert!(painter.textures.contains(&id));
        assert!(pending.all().free.contains(&id));

        // The top-level viewport's paint at the frame's end frees it.
        painter.paint(&[], pending.all());
        assert!(!painter.textures.contains(&id));
        assert!(pending.all().is_empty());
    }

    /// What a paint couldn't give (glow: no texture could be made now) waits for the next one,
    /// before the changes that came after it.
    #[test]
    fn what_a_paint_left_waits_for_the_next_one() {
        let ctx = Context::default();
        let texture = picture(&ctx);
        let mut pending = PendingTextures::default();
        pending.append(ctx.tex_manager().write().take_delta());

        pending.without_frees(|_delta| {
            // Given nothing: left as it was.
        });
        assert!(pending.all().set.contains_key(&texture.id()));

        let mut painter = Painter::default();
        pending.without_frees(|delta| painter.paint(&[], delta));
        assert!(painter.textures.contains(&texture.id()));
        assert!(pending.all().is_empty());
    }
}
