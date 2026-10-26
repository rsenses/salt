use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub struct PopupAction<'a> {
    pub key: &'a str,
    pub label: &'a str,
}

pub struct Popup<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub actions: &'a [PopupAction<'a>],
    pub width: u16,
    pub height: u16,
}

impl Popup<'_> {
    pub fn fits(width: u16, height: u16) -> bool {
        width >= 26 && height >= 9
    }

    /// Draws the shared border, description, and optional action footer.
    /// Returns the area reserved for the popup's specific content.
    pub fn render(&self, frame: &mut Frame<'_>, bounds: Rect) -> Option<Rect> {
        if !Self::fits(bounds.width, bounds.height) {
            return None;
        }
        let width = self.width.min(bounds.width.saturating_sub(2));
        let height = self.height.min(bounds.height.saturating_sub(2));
        if width < 24 || height < 7 {
            return None;
        }
        let area = Rect::new(
            bounds.x + (bounds.width - width) / 2,
            bounds.y + (bounds.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default()
                .title(Line::styled(
                    format!(" {} ", self.title),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ))
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Gray)),
            area,
        );

        let inner_x = area.x + 1;
        let inner_width = area.width - 2;
        let description_height = self.description.lines().count().clamp(1, 2) as u16;
        frame.render_widget(
            Paragraph::new(self.description),
            Rect::new(
                inner_x + 1,
                area.y + 1,
                inner_width.saturating_sub(2),
                description_height,
            ),
        );
        let separator_y = area.y + 1 + description_height;
        frame.render_widget(
            Paragraph::new("─".repeat(inner_width as usize))
                .style(Style::default().fg(Color::Gray)),
            Rect::new(inner_x, separator_y, inner_width, 1),
        );

        let content_y = separator_y + 1;
        let footer_height = if self.actions.is_empty() { 0 } else { 2 };
        let content_end = area.bottom().saturating_sub(1 + footer_height);
        if !self.actions.is_empty() {
            let footer_y = area.bottom() - 2;
            frame.render_widget(
                Paragraph::new("─".repeat(inner_width as usize))
                    .style(Style::default().fg(Color::Gray)),
                Rect::new(inner_x, footer_y - 1, inner_width, 1),
            );
            let slot_width = inner_width / self.actions.len() as u16;
            for (index, action) in self.actions.iter().enumerate() {
                let x = inner_x + slot_width * index as u16;
                let width = if index + 1 == self.actions.len() {
                    inner_width.saturating_sub(slot_width * index as u16)
                } else {
                    slot_width
                };
                let color = match index % 3 {
                    0 => Color::Cyan,
                    1 => Color::Magenta,
                    _ => Color::Red,
                };
                let label = format!("[{}] {}", action.key, action.label);
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        format!(" {label} "),
                        Style::default()
                            .fg(Color::Black)
                            .bg(color)
                            .add_modifier(Modifier::BOLD),
                    )))
                    .alignment(Alignment::Center),
                    Rect::new(x, footer_y, width, 1),
                );
            }
        }
        Some(Rect::new(
            inner_x,
            content_y,
            inner_width,
            content_end.saturating_sub(content_y),
        ))
    }
}

pub struct Alert<'a> {
    pub label: &'a str,
    pub message: &'a str,
}

impl Alert<'_> {
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.is_empty() {
            return;
        }
        let line = Line::from(vec![
            Span::styled(
                format!(" {}: ", self.label),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(self.message),
        ]);
        frame.render_widget(
            Paragraph::new(line).style(Style::default().add_modifier(Modifier::REVERSED)),
            area,
        );
    }
}

pub struct WhichKey<'a> {
    pub title: &'a str,
    pub choices: &'a [(&'a str, &'a str)],
}

impl WhichKey<'_> {
    pub fn render(&self, frame: &mut Frame<'_>, bounds: Rect) {
        let width = bounds.width.saturating_sub(2).min(48);
        let height = bounds.height.saturating_sub(3).min(9);
        if width < 12 || height < 3 {
            return;
        }
        let area = Rect::new(
            bounds.right().saturating_sub(width + 1),
            bounds.bottom().saturating_sub(height + 2),
            width,
            height,
        );
        let lines: Vec<_> = self
            .choices
            .iter()
            .take(height.saturating_sub(2) as usize)
            .map(|(key, action)| {
                Line::from(vec![
                    Span::styled(
                        format!("  {key:<9}"),
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format!(" {action}")),
                ])
            })
            .collect();
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .title(self.title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red)),
            ),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn popup_has_title_description_body_and_action_footer() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                let actions = [
                    PopupAction {
                        key: "y",
                        label: "Yes",
                    },
                    PopupAction {
                        key: "n",
                        label: "No",
                    },
                ];
                let popup = Popup {
                    title: "Delete permanently?",
                    description: "/tmp/example.txt",
                    actions: &actions,
                    width: 60,
                    height: 10,
                };
                let body = popup.render(frame, frame.area()).unwrap();
                frame.render_widget(Paragraph::new("Directory contents will be removed"), body);
            })
            .unwrap();
        assert!(row(&terminal, 7).contains("Delete permanently?"));
        assert!(row(&terminal, 8).contains("/tmp/example.txt"));
        assert!(row(&terminal, 10).contains("Directory contents will be removed"));
        assert!(row(&terminal, 15).contains("[y] Yes"));
        assert!(row(&terminal, 15).contains("[n] No"));
    }

    #[test]
    fn alert_is_reversed_and_which_key_keeps_its_position() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        for choices in [&[("g", "go")][..], &[("g", "go"), ("x", "cut")][..]] {
            terminal
                .draw(|frame| {
                    WhichKey {
                        title: "Next keys",
                        choices,
                    }
                    .render(frame, frame.area());
                    Alert {
                        label: "search",
                        message: "sample",
                    }
                    .render(frame, Rect::new(0, 22, 80, 1));
                    frame.render_widget(Paragraph::new("footer"), Rect::new(0, 23, 80, 1));
                })
                .unwrap();
            assert!(row(&terminal, 13).contains("Next keys"));
            assert!(row(&terminal, 22).contains("search: sample"));
            assert!(row(&terminal, 23).starts_with("footer"));
            assert!(
                terminal.backend().buffer()[(40, 22)]
                    .modifier
                    .contains(Modifier::REVERSED)
            );
        }
    }
}
