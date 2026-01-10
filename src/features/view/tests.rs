#[cfg(test)]
mod tests {
    use crate::features::view::{CalendarDay, CalendarMonth, CalendarWeek, IndexQuery, IndexTemplate, DaySection, FilterViewModel};
    use askama::Template;

    #[test]
    fn test_calendar_rendering() {
        let calendar = vec![
            CalendarMonth {
                name: "January 2026".to_string(),
                weeks: vec![
                    CalendarWeek {
                        days: vec![
                            None,
                            None,
                            None,
                            Some(CalendarDay {
                                day_number: 1,
                                date_iso: "2026-01-01".to_string(),
                                has_events: false,
                                is_today: false,
                            }),
                            Some(CalendarDay {
                                day_number: 2,
                                date_iso: "2026-01-02".to_string(),
                                has_events: true,
                                is_today: false,
                            }),
                            Some(CalendarDay {
                                day_number: 3,
                                date_iso: "2026-01-03".to_string(),
                                has_events: false,
                                is_today: true,
                            }),
                            Some(CalendarDay {
                                day_number: 4,
                                date_iso: "2026-01-04".to_string(),
                                has_events: false,
                                is_today: false,
                            }),
                        ]
                    }
                ]
            }
        ];

        let template = IndexTemplate {
            active_filters: vec![],
            days: vec![],
            is_past_view: false,
            all_event_types: vec![],
            all_sources: vec![],
            all_locations: vec![],
            query: IndexQuery::default(),
            calendar,
        };

        let output = template.render().unwrap();
        
        // Check for key structural elements
        assert!(output.contains("sticky-calendar-header"));
        assert!(output.contains("Jump to Date"));
        assert!(output.contains("January 2026"));
        
        // Check for specific date rendering
        assert!(output.contains("day-2026-01-02"));
        assert!(output.contains("has-events"));
        assert!(output.contains("today"));
    }
}
