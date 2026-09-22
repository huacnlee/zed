use super::Damage;
use crate::Style;

impl Style {
    pub(crate) fn retained_damage(&self, previous: &Self) -> Damage {
        let mut damage = Damage::empty();
        if self.display != previous.display
            || self.overflow != previous.overflow
            || self.scrollbar_width != previous.scrollbar_width
            || self.position != previous.position
            || self.inset != previous.inset
            || self.size != previous.size
            || self.min_size != previous.min_size
            || self.max_size != previous.max_size
            || self.aspect_ratio != previous.aspect_ratio
            || self.margin != previous.margin
            || self.padding != previous.padding
            || self.border_widths != previous.border_widths
            || self.align_items != previous.align_items
            || self.align_self != previous.align_self
            || self.align_content != previous.align_content
            || self.justify_content != previous.justify_content
            || self.gap != previous.gap
            || self.flex_direction != previous.flex_direction
            || self.flex_wrap != previous.flex_wrap
            || self.flex_basis != previous.flex_basis
            || self.flex_grow != previous.flex_grow
            || self.flex_shrink != previous.flex_shrink
            || self.grid_cols != previous.grid_cols
            || self.grid_rows != previous.grid_rows
            || self.grid_location != previous.grid_location
            || self.text.font_family != previous.text.font_family
            || self.text.font_features != previous.text.font_features
            || self.text.font_fallbacks != previous.text.font_fallbacks
            || self.text.font_size != previous.text.font_size
            || self.text.font_weight != previous.text.font_weight
            || self.text.font_style != previous.text.font_style
            || self.text.line_height != previous.text.line_height
            || self.text.white_space != previous.text.white_space
            || self.text.text_overflow != previous.text.text_overflow
            || self.text.line_clamp != previous.text.line_clamp
        {
            damage |= Damage::LAYOUT | Damage::PREPAINT | Damage::PAINT;
        }
        if self.background != previous.background
            || self.border_color != previous.border_color
            || self.border_style != previous.border_style
            || self.corner_radii != previous.corner_radii
            || self.box_shadow != previous.box_shadow
            || self.text.color != previous.text.color
            || self.text.background_color != previous.text.background_color
            || self.text.underline != previous.text.underline
            || self.text.strikethrough != previous.text.strikethrough
            || self.text.text_align != previous.text.text_align
        {
            damage |= Damage::PAINT;
        }
        if self.visibility != previous.visibility || self.mouse_cursor != previous.mouse_cursor {
            damage |= Damage::PREPAINT | Damage::PAINT;
        }
        if self.allow_concurrent_scroll != previous.allow_concurrent_scroll
            || self.restrict_scroll_to_axis != previous.restrict_scroll_to_axis
        {
            damage |= Damage::HANDLERS;
        }
        if self.opacity != previous.opacity {
            // Until scene layers compose opacity, primitives bake it into their colors.
            damage |= Damage::COMPOSITE | Damage::PAINT;
        }
        #[cfg(debug_assertions)]
        if self.debug != previous.debug || self.debug_below != previous.debug_below {
            damage |= Damage::PAINT;
        }
        damage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CursorStyle, Visibility, px, red};

    #[test]
    fn style_changes_are_classified_by_phase() {
        let previous = Style::default();
        let layout_changes: [fn(&mut Style); 7] = [
            |style| style.size.width = px(32.).into(),
            |style| style.padding.left = px(4.).into(),
            |style| style.border_widths.top = px(2.).into(),
            |style| style.flex_grow = 1.,
            |style| style.gap.width = px(4.).into(),
            |style| style.text.font_size = Some(px(20.).into()),
            |style| style.text.line_clamp = Some(2),
        ];
        let paint_changes: [fn(&mut Style); 5] = [
            |style| style.background = Some(red().into()),
            |style| style.border_color = Some(red()),
            |style| style.corner_radii.top_left = px(3.).into(),
            |style| style.text.color = Some(red()),
            |style| style.text.underline = Some(Default::default()),
        ];
        for (changes, expected) in [
            (
                layout_changes.as_slice(),
                Damage::LAYOUT | Damage::PREPAINT | Damage::PAINT,
            ),
            (paint_changes.as_slice(), Damage::PAINT),
        ] {
            for change in changes {
                let mut current = previous.clone();
                change(&mut current);
                assert_eq!(current.retained_damage(&previous), expected);
                assert_eq!(previous.retained_damage(&current), expected);
                assert!(current.retained_damage(&current).is_empty());
            }
        }
        let mut current = previous.clone();
        current.visibility = Visibility::Hidden;
        assert_eq!(
            current.retained_damage(&previous),
            Damage::PREPAINT | Damage::PAINT
        );
        current = previous.clone();
        current.mouse_cursor = Some(CursorStyle::PointingHand);
        assert_eq!(
            current.retained_damage(&previous),
            Damage::PREPAINT | Damage::PAINT
        );
        current = previous.clone();
        current.allow_concurrent_scroll = !previous.allow_concurrent_scroll;
        assert_eq!(current.retained_damage(&previous), Damage::HANDLERS);
        current = previous.clone();
        current.opacity = Some(0.5);
        assert_eq!(
            current.retained_damage(&previous),
            Damage::COMPOSITE | Damage::PAINT
        );
    }
}
