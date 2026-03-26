use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
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
        b.iter(|| {
            parse_xml(black_box(&xml)).unwrap()
        })
    });
}

fn bench_parse_medium(c: &mut Criterion) {
    let xml = generate_xml(4, 5); // 625 nodes
    
    c.bench_function("parse_medium_xml", |b| {
        b.iter(|| {
            parse_xml(black_box(&xml)).unwrap()
        })
    });
}

fn bench_parse_large(c: &mut Criterion) {
    let xml = generate_xml(5, 5); // 3125 nodes
    
    c.bench_function("parse_large_xml", |b| {
        b.iter(|| {
            parse_xml(black_box(&xml)).unwrap()
        })
    });
}

fn bench_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialize");
    
    for size in [3, 4, 5].iter() {
        let xml = generate_xml(*size, 3);
        let doc = parse_xml(&xml).unwrap();
        
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                serialize_xml(black_box(&doc)).unwrap()
            })
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
        b.iter(|| {
            parse_xml(black_box(xml)).unwrap()
        })
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
        b.iter(|| {
            parse_xml(black_box(xml)).unwrap()
        })
    });
}

criterion_group!(
    benches,
    bench_parse_small,
    bench_parse_medium,
    bench_parse_large,
    bench_serialize,
    bench_parse_with_attributes,
    bench_real_world_xml
);

criterion_main!(benches);
