use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use xml_tool::exi::{decode_exi_to_xml, encode_xml_to_exi};
use xml_tool::ui::syntax_highlighter::SyntaxHighlighter;
use xml_tool::ui::theme::Palette;
use xml_tool::xml::{parse_xml, serialize_xml};

fn generate_xml(depth: usize, breadth: usize) -> String {
    fn generate_node(depth: usize, breadth: usize, current_depth: usize) -> String {
        if current_depth >= depth {
            return String::from("<leaf>text content</leaf>");
        }

        let mut children = String::new();
        for i in 0..breadth {
            children.push_str(&format!(
                "<child_{}>{}",
                i,
                generate_node(depth, breadth, current_depth + 1)
            ));
            children.push_str(&format!("</child_{}>", i));
        }

        children
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<root>{}</root>"#,
        generate_node(depth, breadth, 0)
    )
}

fn bench_parse_small(c: &mut Criterion) {
    let xml = generate_xml(3, 3); // 27 nodes

    c.bench_function("parse_small_xml", |b| {
        b.iter(|| parse_xml(black_box(&xml)).unwrap())
    });
}

fn bench_parse_medium(c: &mut Criterion) {
    let xml = generate_xml(4, 5); // 625 nodes

    c.bench_function("parse_medium_xml", |b| {
        b.iter(|| parse_xml(black_box(&xml)).unwrap())
    });
}

fn bench_parse_large(c: &mut Criterion) {
    let xml = generate_xml(5, 5); // 3125 nodes

    c.bench_function("parse_large_xml", |b| {
        b.iter(|| parse_xml(black_box(&xml)).unwrap())
    });
}

fn bench_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialize");

    for size in [3, 4, 5].iter() {
        let xml = generate_xml(*size, 3);
        let doc = parse_xml(&xml).unwrap();

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| serialize_xml(black_box(&doc)).unwrap())
        });
    }

    group.finish();
}

fn bench_parse_with_attributes(c: &mut Criterion) {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<root>
    <item id="1" name="first" type="test" value="100">Content 1</item>
    <item id="2" name="second" type="test" value="200">Content 2</item>
    <item id="3" name="third" type="test" value="300">Content 3</item>
</root>"#;

    c.bench_function("parse_with_attributes", |b| {
        b.iter(|| parse_xml(black_box(xml)).unwrap())
    });
}

fn bench_real_world_xml(c: &mut Criterion) {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<company name="TechCorp" founded="2010">
  <headquarters>
    <address type="main">
      <street>123 Innovation Drive</street>
      <city>San Francisco</city>
      <state>California</state>
      <zip>94102</zip>
      <country>USA</country>
    </address>
  </headquarters>
  <departments>
    <department id="eng" budget="5000000">
      <name>Engineering</name>
      <manager>John Smith</manager>
      <employees>
        <employee id="e001">
          <name>Alice Johnson</name>
          <title>Senior Engineer</title>
          <salary currency="USD">150000</salary>
          <skills>
            <skill>Rust</skill>
            <skill>Python</skill>
            <skill>System Design</skill>
          </skills>
        </employee>
      </employees>
    </department>
  </departments>
</company>"#;

    c.bench_function("parse_real_world", |b| {
        b.iter(|| parse_xml(black_box(xml)).unwrap())
    });
}

fn bench_tree_search(c: &mut Criterion) {
    let xml = generate_xml(5, 5);
    let doc = XmlDocument::parse(xml.as_bytes()).expect("parse");
    let order = doc.document_order().to_vec();
    let mut group = c.benchmark_group("tree_search");

    group.bench_function("cold_leaf_query", |b| {
        b.iter_batched(
            || SearchIndex::build(&doc),
            |mut index| black_box(index.search("leaf", false, &order).len()),
            BatchSize::SmallInput,
        )
    });

    let mut warm_index = SearchIndex::build(&doc);
    group.bench_function("warm_leaf_query", |b| {
        b.iter(|| black_box(warm_index.search("leaf", false, &order).len()))
    });

    group.finish();
}

fn bench_highlight_xml(c: &mut Criterion) {
    let xml = generate_xml(5, 5);
    let highlighter = SyntaxHighlighter::new();
    let palette = bench_palette();

    c.bench_function("highlight_large_xml", |b| {
        b.iter(|| black_box(highlighter.highlight_xml_lines(black_box(&palette), black_box(&xml))))
    });
}

fn bench_exi_round_trip(c: &mut Criterion) {
    let xml = generate_xml(4, 4);
    let exi = encode_xml_to_exi(&xml).unwrap();
    let mut group = c.benchmark_group("exi");

    group.bench_function("encode_medium_xml", |b| {
        b.iter(|| encode_xml_to_exi(black_box(&xml)).unwrap())
    });
    group.bench_function("decode_medium_exi", |b| {
        b.iter(|| decode_exi_to_xml(black_box(exi.as_slice())).unwrap())
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_parse_small,
    bench_parse_medium,
    bench_parse_large,
    bench_serialize,
    bench_parse_with_attributes,
    bench_real_world_xml,
    bench_tree_search,
    bench_highlight_xml,
    bench_exi_round_trip
);

// ---------------------------------------------------------------------------
// Step 9 benchmark groups: the plan's named performance surfaces
// ---------------------------------------------------------------------------

use xml_tool::core::document::XmlDocument;
use xml_tool::core::{Command, History};
use xml_tool::fixtures;
use xml_tool::services::exi_workbench::{ExiPreset, ExiSettings, encode_with_settings};
use xml_tool::services::outline::FlatTree;
use xml_tool::services::search::SearchIndex;

fn bench_parse_20_mib(c: &mut Criterion) {
    let bytes = fixtures::large_bytes_xml();
    c.bench_function("parse_20_mib", |b| {
        b.iter(|| XmlDocument::parse(black_box(bytes.as_bytes())).unwrap())
    });
}

fn bench_serialize_20_mib(c: &mut Criterion) {
    let bytes = fixtures::large_bytes_xml();
    let document = XmlDocument::parse(bytes.as_bytes()).unwrap();
    c.bench_function("serialize_20_mib", |b| {
        b.iter(|| {
            black_box(
                xml_tool::xml::encoding::encode_xml_text(
                    black_box(document.source()),
                    xml_tool::xml::SourceEncoding::Utf8,
                )
                .len(),
            )
        })
    });
}

fn bench_search_200k(c: &mut Criterion) {
    let nodes = fixtures::large_nodes_xml();
    let document = XmlDocument::parse(nodes.as_bytes()).unwrap();
    let order = document.document_order().to_vec();
    c.bench_function("search_200k_first", |b| {
        b.iter_batched(
            || SearchIndex::build(&document),
            |mut index| black_box(index.search("id=\"123456\"", false, &order).len()),
            BatchSize::LargeInput,
        )
    });
    let mut index = SearchIndex::build(&document);
    let mut group = c.benchmark_group("search_200k");
    group.bench_function("cached", |b| {
        b.iter(|| black_box(index.search("id=\"123456\"", false, &order).len()))
    });
    group.finish();
}

fn bench_incremental_edits(c: &mut Criterion) {
    let mut group = c.benchmark_group("edits_20_mib");
    group.bench_function("edit_and_undo", |b| {
        b.iter_batched(
            || {
                let bytes = fixtures::large_bytes_xml();
                let document = XmlDocument::parse(bytes.as_bytes()).unwrap();
                let root = document.root_element().unwrap();
                let first_record = document
                    .children(root)
                    .into_iter()
                    .find(|id| document.kind(*id) == Some(xml_tool::core::XmlNodeKind::Element))
                    .unwrap();
                let title = document
                    .children(first_record)
                    .into_iter()
                    .find(|id| document.kind(*id) == Some(xml_tool::core::XmlNodeKind::Element))
                    .unwrap();
                let text = document.children(title)[0];
                (document, text)
            },
            |(mut document, text)| {
                let mut history = History::new();
                history
                    .commit(
                        &mut document,
                        Command::SetNodeContent {
                            node: text,
                            content: xml_tool::core::NodeContent::Text("bench".into()),
                        },
                    )
                    .unwrap();
                history.undo(&mut document).unwrap();
            },
            BatchSize::LargeInput,
        )
    });
    group.finish();
}

fn bench_flat_tree_toggle(c: &mut Criterion) {
    let nodes = fixtures::large_nodes_xml();
    let document = XmlDocument::parse(nodes.as_bytes()).unwrap();
    let root = document.root_element().unwrap();
    let mut expanded = std::collections::HashSet::new();
    expanded.insert(root);
    let mut group = c.benchmark_group("flat_tree");
    group.bench_function("build_expanded_200k", |b| {
        b.iter(|| black_box(FlatTree::build(black_box(&document), &expanded).len()))
    });
    group.finish();
}

fn bench_visible_highlight(c: &mut Criterion) {
    let bytes = fixtures::large_bytes_xml();
    let window: String = bytes
        .lines()
        .skip(9_900)
        .take(200)
        .collect::<Vec<_>>()
        .join("\n");
    let highlighter = SyntaxHighlighter::new();
    let palette = bench_palette();
    let mut group = c.benchmark_group("highlight");
    group.bench_function("visible_200_lines", |b| {
        b.iter(|| {
            black_box(highlighter.highlight_xml_lines(black_box(&palette), black_box(&window)))
        })
    });
    group.finish();
}

fn bench_exi_presets(c: &mut Criterion) {
    let sample = generate_xml(4, 5);
    let mut group = c.benchmark_group("exi_presets");
    for preset in [
        ExiPreset::FidelityBitPacked,
        ExiPreset::ByteAligned,
        ExiPreset::PreCompression,
        ExiPreset::MaximumCompression,
    ] {
        let settings = ExiSettings::preset(preset);
        group.bench_function(format!("{preset:?}"), |b| {
            b.iter(|| {
                black_box(
                    encode_with_settings(black_box(sample.as_str()), &settings)
                        .unwrap()
                        .0
                        .len(),
                )
            })
        });
    }
    group.finish();
}

criterion_group!(
    step9_benches,
    bench_parse_20_mib,
    bench_serialize_20_mib,
    bench_search_200k,
    bench_incremental_edits,
    bench_flat_tree_toggle,
    bench_visible_highlight,
    bench_exi_presets
);
criterion_main!(benches, step9_benches);

/// A fixed dark palette for highlighting benchmarks (theme resolution is
/// not the hot path being measured).
fn bench_palette() -> Palette {
    let ctx = egui::Context::default();
    ctx.set_theme(egui::ThemePreference::Dark);
    Palette::resolve(&ctx)
}
