use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use xml_tool::exi::{decode_exi_to_xml, encode_xml_to_exi};
use xml_tool::ui::syntax_highlighter::SyntaxHighlighter;
use xml_tool::ui::xml_tree::XmlTreeView;
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
    let doc = parse_xml(&xml).unwrap();
    let mut group = c.benchmark_group("tree_search");

    group.bench_function("cold_leaf_query", |b| {
        b.iter_batched(
            XmlTreeView::new,
            |mut view| {
                black_box(view.search_match_count(doc.root.as_ref(), doc.version(), "leaf", false))
            },
            BatchSize::SmallInput,
        )
    });

    let mut warm_view = XmlTreeView::new();
    warm_view.search_match_count(doc.root.as_ref(), doc.version(), "leaf", false);
    group.bench_function("warm_leaf_query", |b| {
        b.iter(|| {
            black_box(warm_view.search_match_count(doc.root.as_ref(), doc.version(), "leaf", false))
        })
    });

    group.finish();
}

fn bench_highlight_xml(c: &mut Criterion) {
    let xml = generate_xml(5, 5);
    let highlighter = SyntaxHighlighter::new();

    c.bench_function("highlight_large_xml", |b| {
        b.iter(|| black_box(highlighter.highlight_xml_lines(black_box(&xml))))
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

criterion_main!(benches);
