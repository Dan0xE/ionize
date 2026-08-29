<!-- SPDX-License-Identifier: MPL-2.0 -->

# ionize_draw

`ionize_draw` renders an `ionize::Graph` as SVG. 

```rust
use ionize::{Graph, Node, Opts, Size};
use ionize_draw::{Block, Cell, Col, Diagram, Renderer, Row, Table};

# fn example() -> ionize_draw::Result<()> {
let mut graph = Graph::new();
graph.add(Node::new(Size::default()));

let mut table = Table::new([Col::normal(0.0)]);
table.push(Row::new([Cell::new("return")]));
let diagram = Diagram::new(graph, vec![Block::new("Exit", table)])?;
let svg = Renderer::new()?.render(diagram, &Opts::default())?;
assert!(svg.starts_with("<?xml"));
# Ok(())
# }
```

