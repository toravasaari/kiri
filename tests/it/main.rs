//! The integration tests, built as one test binary.
//!
//! One binary links once, where each file linked on its own, and its tests
//! run in one pool. Each module is one file of this directory: its tests,
//! and the snapshots in `snapshots/`. `common` holds the helpers they share.

mod cli_completions;
mod cli_config;
mod cli_csv;
mod cli_diagrams;
mod cli_doctor;
mod cli_document;
mod cli_files;
mod cli_formulas;
mod cli_images;
mod cli_list;
mod cli_pager;
mod cli_reading;
mod cli_routes;
mod cli_stdin;
mod cli_text;
mod cli_tmux;
mod common;
mod examples;
mod guard_direction;
mod guard_line_caps;
mod render_alerts;
mod render_blocks;
mod render_diagrams;
mod render_emoji;
mod render_files;
mod render_front_matter;
mod render_headings;
mod render_html_tags;
mod render_image_formats;
mod render_images;
mod render_images_html;
mod render_pager;
mod render_safety;
mod render_tables;
mod render_themes;
