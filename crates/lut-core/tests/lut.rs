use lut_core::*;

fn close(a: Rgb, b: Rgb, eps: f32) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() <= eps)
}

#[test]
fn default_doc_bakes_identity() {
    let t = LutDoc::default().bake();
    assert_eq!(t.size, LUT_SIZE);
    let id = Table::identity(LUT_SIZE);
    for (a, b) in t.data.iter().zip(&id.data) {
        assert!(close(*a, *b, 1e-5), "{a:?} vs {b:?}");
    }
}

#[test]
fn table_order_is_red_fastest() {
    let t = Table::identity(3);
    assert_eq!(t.data[1], [0.5, 0.0, 0.0]);
    assert_eq!(t.data[3], [0.0, 0.5, 0.0]);
    assert_eq!(t.data[9], [0.0, 0.0, 0.5]);
}

#[test]
fn identity_sample_is_exact_between_points() {
    let t = Table::identity(17);
    for c in [[0.13f32, 0.5, 0.97], [0.0, 1.0, 0.31], [1.2, -0.1, 0.5]] {
        let want = [
            c[0].clamp(0.0, 1.0),
            c[1].clamp(0.0, 1.0),
            c[2].clamp(0.0, 1.0),
        ];
        assert!(close(t.sample(c), want, 1e-5));
    }
}

#[test]
fn controls_move_colours_the_expected_way() {
    let grey = [0.5, 0.5, 0.5];
    let g = Grade {
        exposure: 1.0,
        ..Grade::default()
    };
    assert!(close(g.apply([0.25, 0.25, 0.25]), grey, 1e-5));

    let warm = Grade {
        temperature: 1.0,
        ..Grade::default()
    }
    .apply(grey);
    assert!(warm[0] > 0.5 && warm[2] < 0.5);

    let mono = Grade {
        saturation: 0.0,
        ..Grade::default()
    }
    .apply([1.0, 0.0, 0.0]);
    assert!(close(mono, [0.2126; 3], 1e-5));

    let flat = Grade {
        contrast: 0.0,
        ..Grade::default()
    }
    .apply([0.1, 0.9, 0.3]);
    assert!(close(flat, grey, 1e-5));

    let lifted = Grade {
        lift: Wheel {
            master: 0.1,
            r: 0.0,
            g: 0.0,
            b: 0.0,
        },
        ..Grade::default()
    }
    .apply([0.0; 3]);
    assert!(close(lifted, [0.1; 3], 1e-5));

    let red_gain = Grade {
        gain: Wheel {
            master: 1.0,
            r: 2.0,
            g: 1.0,
            b: 1.0,
        },
        ..Grade::default()
    }
    .apply([0.25; 3]);
    assert!(close(red_gain, [0.5, 0.25, 0.25], 1e-5));

    let gamma = Grade {
        gamma: Wheel::uniform(1.0).with_master(2.0),
        ..Grade::default()
    }
    .apply([0.25; 3]);
    assert!(close(gamma, [0.5; 3], 1e-5));
}

trait WithMaster {
    fn with_master(self, m: f32) -> Self;
}
impl WithMaster for Wheel {
    fn with_master(mut self, m: f32) -> Self {
        self.master = m;
        self
    }
}

#[test]
fn output_is_always_in_range() {
    let g = Grade {
        exposure: 4.0,
        contrast: 3.0,
        saturation: 3.0,
        lift: Wheel::uniform(-0.5),
        ..Grade::default()
    };
    let doc = LutDoc {
        grade: g,
        ..LutDoc::default()
    };
    for v in doc.bake().data {
        assert!(v.iter().all(|x| (0.0..=1.0).contains(x)), "{v:?}");
    }
}

#[test]
fn curve_passes_through_points_and_stays_monotone() {
    let c = Curve {
        points: vec![
            [0.0, 0.0],
            [0.25, 0.1],
            [0.5, 0.6],
            [0.75, 0.62],
            [1.0, 1.0],
        ],
    };
    let p = c.prepare();
    for [x, y] in &c.points {
        assert!((p.eval(*x) - y).abs() < 1e-5);
    }
    let mut last = -1.0;
    for i in 0..=1000 {
        let y = p.eval(i as f32 / 1000.0);
        assert!(y >= last - 1e-6, "not monotone at {i}");
        last = y;
    }
}

#[test]
fn curve_edges_hold_and_unsorted_points_work() {
    let c = Curve {
        points: vec![[0.8, 0.9], [0.2, 0.3]],
    };
    let p = c.prepare();
    assert!((p.eval(0.0) - 0.3).abs() < 1e-6);
    assert!((p.eval(1.0) - 0.9).abs() < 1e-6);
    assert!(Curve::default().is_identity());
    assert!(!c.is_identity());
}

#[test]
fn master_curve_then_channel_curve() {
    let mut doc = LutDoc::default();
    doc.curves.master = Curve {
        points: vec![[0.0, 0.0], [1.0, 0.5]],
    };
    doc.curves.r = Curve {
        points: vec![[0.0, 1.0], [1.0, 0.0]],
    }; // inverts red after the master
    let t = doc.bake();
    let white = t.data[t.data.len() - 1];
    assert!(close(white, [0.5, 0.5, 0.5], 1e-4), "{white:?}");
    let black = t.data[0];
    assert!(close(black, [1.0, 0.0, 0.0], 1e-4), "{black:?}");
}

#[test]
fn cube_round_trip() {
    let doc = LutDoc {
        grade: Grade {
            temperature: 0.6,
            saturation: 1.4,
            ..Grade::default()
        },
        ..LutDoc::default()
    };
    let t = doc.bake();
    let text = write_cube("Warm \"stage\"", &t);
    assert!(text.starts_with("TITLE \"Warm stage\"\n"));
    let (title, back) = parse_cube(&text).unwrap();
    assert_eq!(title.as_deref(), Some("Warm stage"));
    assert_eq!(back.size, t.size);
    for (a, b) in back.data.iter().zip(&t.data) {
        assert!(close(*a, *b, 1e-6));
    }
}

#[test]
fn cube_1d_becomes_3d() {
    let text = "# comment\nLUT_1D_SIZE 2\n1 0 0\n0 1 1\n";
    let (_, t) = parse_cube(text).unwrap();
    assert_eq!(t.size, LUT_SIZE);
    // red inverted, green and blue unchanged
    assert!(close(
        t.sample([0.25, 0.25, 0.75]),
        [0.75, 0.25, 0.75],
        1e-5
    ));
}

#[test]
fn cube_domain_is_mapped_to_0_1() {
    // A 2-point identity over 0..2: input 0.5 sits at a quarter of the grid, i.e. output 0.5.
    let text = "LUT_3D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n0 0 0\n2 0 0\n0 2 0\n2 2 0\n0 0 2\n2 0 2\n0 2 2\n2 2 2\n";
    let (_, t) = parse_cube(text).unwrap();
    assert!(close(t.sample([0.5, 0.25, 1.0]), [0.5, 0.25, 1.0], 1e-5));
}

#[test]
fn cube_errors() {
    assert_eq!(parse_cube("1 2 3\n").unwrap_err(), CubeError::NoSize);
    assert!(matches!(
        parse_cube("LUT_3D_SIZE 2\n0 0 0\n").unwrap_err(),
        CubeError::Count {
            expected: 8,
            found: 1
        }
    ));
    assert!(matches!(
        parse_cube("LUT_3D_SIZE 2\n0 0\n").unwrap_err(),
        CubeError::Syntax { line: 2, .. }
    ));
    assert_eq!(
        parse_cube("LUT_3D_SIZE 1\n").unwrap_err(),
        CubeError::BadSize(1)
    );
    assert_eq!(
        parse_cube("LUT_1D_SIZE 2\nDOMAIN_MAX 0 1 1\n0 0 0\n1 1 1\n").unwrap_err(),
        CubeError::BadDomain
    );
}

#[test]
fn imported_base_with_controls_on_top() {
    let (_, inv) = parse_cube(&write_cube(
        "inv",
        &Table::from_fn(9, |c| [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]]),
    ))
    .unwrap();
    let mut doc = LutDoc::from_base(inv);
    assert_eq!(doc.base.as_ref().unwrap().size, LUT_SIZE);
    assert!(close(
        doc.bake().sample([0.2, 0.3, 0.4]),
        [0.8, 0.7, 0.6],
        1e-4
    ));
    doc.grade.saturation = 0.0;
    let y = 0.2126 * 0.8 + 0.7152 * 0.7 + 0.0722 * 0.6;
    assert!(close(doc.bake().sample([0.2, 0.3, 0.4]), [y; 3], 1e-4));
}

#[test]
fn doc_json_round_trip_and_version() {
    let mut doc = LutDoc::default();
    doc.grade.tint = -0.3;
    doc.curves.g.points.push([0.5, 0.6]);
    let back = LutDoc::from_json(&doc.to_json()).unwrap();
    assert_eq!(back, doc);
    // Missing fields take their defaults.
    assert_eq!(
        LutDoc::from_json(r#"{"version":1}"#).unwrap(),
        LutDoc::default()
    );
    assert!(matches!(
        LutDoc::from_json(r#"{"version":2}"#),
        Err(LutDocError::TooNew(2))
    ));
    assert!(matches!(
        LutDoc::from_json(r#"{"version":1,"base":{"size":2,"data":[[0,0,0]]}}"#),
        Err(LutDocError::BadBase(_))
    ));
}

#[test]
fn lut_names() {
    for ok in ["Warm Stage", "LED wall 2", "Größe", "a.b"] {
        assert!(valid_lut_name(ok), "{ok}");
    }
    for bad in [
        "",
        " x",
        "x ",
        "x.",
        "a/b",
        "a:b",
        "CON",
        "nul",
        "COM1",
        "lpt9.cube",
        &"x".repeat(65),
    ] {
        assert!(!valid_lut_name(bad), "{bad}");
    }
}

#[test]
fn f32_bytes_layout() {
    let b = Table::identity(2).to_f32_le_bytes();
    assert_eq!(b.len(), 8 * 12);
    assert_eq!(f32::from_le_bytes(b[12..16].try_into().unwrap()), 1.0); // entry 1 = red
}
