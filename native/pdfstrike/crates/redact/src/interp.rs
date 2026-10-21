//! The content interpreter that applies (or verifies) redaction on one content scope: a page's
//! streams or a form XObject's.
//!
//! It tracks just enough graphics state to place every glyph, image and path in user space:
//! the CTM stack, the text state (font, size, spacing, scaling, rise, leading) and the text
//! matrices. In apply mode it rewrites the operators:
//! - text: glyphs whose boxes overlap a region are cut out of `Tj`/`TJ`/`'`/`"`, replaced by a
//!   `TJ` displacement of the same width so the remaining text stays exactly where it was;
//! - images: fully covered → the `Do` is removed; partly covered → the covered pixels are
//!   cleared in a copy of the image (or the image is removed when its codec can't be re-encoded);
//!   inline images under a region are removed;
//! - vectors: fully covered paths are removed; partly covered ones are clipped so nothing shows
//!   inside the regions; shadings are clipped the same way;
//! - form XObjects under a region are rewritten recursively into new objects (copy-on-write, so
//!   other pages using the original are untouched).
//!
//! In verify mode nothing changes; glyphs and inline images that still overlap a region are
//! counted.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use pdfcraft_content::{Matrix, Op, Pieces, contains, num, overlaps, serialize_ops, string};
use pdfcraft_cos::{Dict, Document, ObjRef, Object, Stream};

use crate::{RedactError, Report, image};
use pdfcraft_fonts::pdf::Metrics;

const MAX_DEPTH: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Apply,
    Verify,
}

#[derive(Clone)]
struct Gs {
    ctm: Matrix,
    font: Rc<Metrics>,
    font_known: bool,
    size: f64,
    char_spacing: f64,
    word_spacing: f64,
    render_mode: i64,
    scale: f64,
    leading: f64,
    rise: f64,
}

/// What processing a scope produced.
#[derive(Default)]
pub(crate) struct Output {
    /// The rewritten streams (`None` = unchanged).
    pub streams: Vec<Option<Vec<u8>>>,
    /// New XObjects the rewritten content refers to: (resource name, object).
    pub xobjects: Vec<(Vec<u8>, ObjRef)>,
    /// Original XObjects replaced or dropped, including nested scopes.
    pub doomed: Vec<ObjRef>,
    /// Resource cleanup must fail closed when surviving uses cannot be established.
    pub error: Option<RedactError>,
    /// Glyphs / inline images still overlapping a region (verify mode).
    pub residue: usize,
}

pub(crate) struct Scope<'a> {
    pub rects: &'a [[f64; 4]],
    pub mode: Mode,
    pub report: &'a mut Report,
    /// Sanitize: remove hidden text (invisible render modes, or wholly outside this page box).
    pub hidden_text: Option<[f64; 4]>,
    /// Sanitize: optional content groups that are off; content marked with them is removed.
    pub hidden_layers: HashSet<ObjRef>,
    /// Whether this pass is evaluating optional-content rules. An empty off set is meaningful:
    /// policies such as `/AllOff` can still hide content when every group is on.
    pub layers_enabled: bool,
    /// Off groups referenced by optional content that remains visible and therefore still needs
    /// its original default state after sanitization.
    pub retained_off_layers: HashSet<ObjRef>,
    /// Cache membership results so repeated blocks cannot repeatedly walk hostile expressions.
    ocmd_cache: HashMap<ObjRef, bool>,
    /// Direct `/OC` dictionaries on XObjects have no object reference of their own. Cache them
    /// by the containing XObject so repeated `Do` calls share the same bounded evaluation.
    xobject_oc_cache: HashMap<ObjRef, bool>,
    /// Content removed from hidden layers (blocks and XObjects).
    pub layer_blocks: usize,
    fonts: HashMap<Vec<u8>, Rc<Metrics>>,
    used_names: Vec<Vec<u8>>,
    depth: usize,
}

impl<'a> Scope<'a> {
    pub fn new(rects: &'a [[f64; 4]], mode: Mode, report: &'a mut Report) -> Self {
        Scope {
            rects,
            mode,
            report,
            hidden_text: None,
            hidden_layers: HashSet::new(),
            layers_enabled: false,
            retained_off_layers: HashSet::new(),
            ocmd_cache: HashMap::new(),
            xobject_oc_cache: HashMap::new(),
            layer_blocks: 0,
            fonts: HashMap::new(),
            used_names: Vec::new(),
            depth: 0,
        }
    }

    /// Sanitizing visits every form XObject (not only those under a region).
    fn everywhere(&self) -> bool {
        self.hidden_text.is_some() || self.layers_enabled
    }

    /// Is this optional-content reference (an OCG or OCMD) hidden?
    fn layer_hidden(&mut self, doc: &Document, o: &Object) -> bool {
        if !self.layers_enabled {
            return false;
        }
        let reference = o.as_ref();
        if let Some(r) = reference {
            if let Some(hidden) = self.ocmd_cache.get(&r) {
                return *hidden;
            }
            if self.hidden_layers.contains(&r) {
                self.ocmd_cache.insert(r, true);
                return true;
            }
        }
        // Evaluate the policy against the same default configuration used to select hidden
        // groups. A membership dictionary's default policy is AnyOn.
        let d = doc.resolve(o);
        let Some(d) = d.as_dict() else { return false };
        if !type_is(doc, d, b"OCMD") {
            return false;
        }
        let groups: Vec<ObjRef> = match d.get(b"OCGs").map(|g| (*doc.resolve(g)).clone()) {
            Some(Object::Array(a)) => a.iter().filter_map(Object::as_ref).collect(),
            Some(_) => d.get(b"OCGs").and_then(Object::as_ref).into_iter().collect(),
            None => Vec::new(),
        };
        let fail_closed = || groups.iter().any(|g| self.hidden_layers.contains(g));
        let hidden = if let Some(expression) = d.get(b"VE") {
            let visible = self.visibility_expression(doc, expression, 0, &mut HashSet::new(), &mut 4096);
            match visible {
                Some(visible) => !visible,
                None => fail_closed() || self.expression_mentions_hidden(doc, expression, 0, &mut HashSet::new(), &mut 4096),
            }
        } else if groups.is_empty() {
            false
        } else {
            let any_on = groups.iter().any(|g| !self.hidden_layers.contains(g));
            let all_on = groups.iter().all(|g| !self.hidden_layers.contains(g));
            let policy = d.get(b"P").map(|p| doc.resolve(p)).and_then(|p| p.as_name().map(|name| name.to_vec()));
            match (d.get(b"P"), policy.as_deref()) {
                (None, _) | (Some(_), Some(b"AnyOn")) => !any_on,
                (Some(_), Some(b"AllOn")) => !all_on,
                (Some(_), Some(b"AnyOff")) => all_on,
                (Some(_), Some(b"AllOff")) => any_on,
                // An invalid policy must not make a candidate hidden block survive sanitization.
                _ => fail_closed(),
            }
        };
        if !hidden {
            self.retained_off_layers.extend(groups.iter().filter(|g| self.hidden_layers.contains(g)).copied());
            if let Some(expression) = d.get(b"VE") {
                let mut referenced = HashSet::new();
                if Self::collect_hidden_expression_groups(doc, expression, &self.hidden_layers, 0, &mut HashSet::new(), &mut 4096, &mut referenced) {
                    self.retained_off_layers.extend(referenced);
                } else {
                    // If an expression cannot be fully walked, preserve every off group so that
                    // the content we kept does not change visibility after configuration cleanup.
                    self.retained_off_layers.extend(self.hidden_layers.iter().copied());
                }
            }
        }
        if let Some(r) = reference {
            self.ocmd_cache.insert(r, hidden);
        }
        hidden
    }

    fn xobject_layer_hidden(&mut self, doc: &Document, xobject: ObjRef, oc: &Object) -> bool {
        if oc.as_ref().is_some() {
            return self.layer_hidden(doc, oc);
        }
        if let Some(hidden) = self.xobject_oc_cache.get(&xobject) {
            return *hidden;
        }
        let hidden = self.layer_hidden(doc, oc);
        self.xobject_oc_cache.insert(xobject, hidden);
        hidden
    }

    fn collect_hidden_expression_groups(
        doc: &Document,
        expression: &Object,
        hidden_layers: &HashSet<ObjRef>,
        depth: usize,
        path: &mut HashSet<ObjRef>,
        budget: &mut usize,
        found: &mut HashSet<ObjRef>,
    ) -> bool {
        if depth > 64 || *budget == 0 {
            return false;
        }
        *budget -= 1;
        let reference = expression.as_ref();
        if let Some(r) = reference
            && !path.insert(r)
        {
            return false;
        }
        let resolved = doc.resolve(expression);
        let complete = match &*resolved {
            Object::Dict(d) if type_is(doc, d, b"OCG") => {
                if let Some(r) = reference
                    && hidden_layers.contains(&r)
                {
                    found.insert(r);
                }
                reference.is_some()
            }
            Object::Array(items) => {
                let Some(operator) = items.first().map(|o| doc.resolve(o)) else { return false };
                let Some(operator) = operator.as_name() else { return false };
                let operands = match operator {
                    b"And" | b"Or" if (2..=4097).contains(&items.len()) => items.get(1..),
                    b"Not" if items.len() == 2 => items.get(1..),
                    _ => None,
                };
                operands.is_some_and(|operands| {
                    operands.iter().all(|item| Self::collect_hidden_expression_groups(doc, item, hidden_layers, depth + 1, path, budget, found))
                })
            }
            _ => false,
        };
        if let Some(r) = reference {
            path.remove(&r);
        }
        complete
    }

    /// Evaluate an optional-content visibility expression (`/VE`) against the default config.
    /// Malformed, cyclic, or excessive expressions return `None` so the caller can fail closed.
    fn visibility_expression(
        &self,
        doc: &Document,
        expression: &Object,
        depth: usize,
        path: &mut HashSet<ObjRef>,
        budget: &mut usize,
    ) -> Option<bool> {
        if depth > 64 || *budget == 0 {
            return None;
        }
        *budget -= 1;
        let reference = expression.as_ref();
        if let Some(r) = reference
            && !path.insert(r)
        {
            return None;
        }
        let resolved = doc.resolve(expression);
        let result = match &*resolved {
            Object::Dict(d) if type_is(doc, d, b"OCG") => reference.map(|r| !self.hidden_layers.contains(&r)),
            Object::Array(items) if (2..=4097).contains(&items.len()) => {
                let operator = doc.resolve(items.first()?);
                let operator = operator.as_name()?;
                match operator {
                    b"And" if items.len() >= 2 => {
                        let mut visible = true;
                        for item in items.iter().skip(1) {
                            visible &= self.visibility_expression(doc, item, depth + 1, path, budget)?;
                        }
                        Some(visible)
                    }
                    b"Or" if items.len() >= 2 => {
                        let mut visible = false;
                        for item in items.iter().skip(1) {
                            visible |= self.visibility_expression(doc, item, depth + 1, path, budget)?;
                        }
                        Some(visible)
                    }
                    b"Not" if items.len() == 2 => Some(!self.visibility_expression(doc, &items[1], depth + 1, path, budget)?),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(r) = reference {
            path.remove(&r);
        }
        result
    }

    /// Conservatively find hidden groups in an expression that could not be evaluated. Unknown
    /// syntax, cycles, and exhausted budgets count as a possible hidden reference.
    fn expression_mentions_hidden(&self, doc: &Document, expression: &Object, depth: usize, path: &mut HashSet<ObjRef>, budget: &mut usize) -> bool {
        if depth > 64 || *budget == 0 {
            return true;
        }
        *budget -= 1;
        if let Some(r) = expression.as_ref() {
            if self.hidden_layers.contains(&r) {
                return true;
            }
            if !path.insert(r) {
                return true;
            }
        }
        let resolved = doc.resolve(expression);
        let found = match &*resolved {
            Object::Array(items) => items.iter().any(|item| self.expression_mentions_hidden(doc, item, depth + 1, path, budget)),
            Object::Dict(d) if type_is(doc, d, b"OCG") => expression.as_ref().is_none_or(|r| self.hidden_layers.contains(&r)),
            Object::Dict(_) => true,
            _ => false,
        };
        if let Some(r) = expression.as_ref() {
            path.remove(&r);
        }
        found
    }

    fn hits(&self, b: [f64; 4]) -> bool {
        self.rects.iter().any(|r| overlaps(*r, b, 0.0))
    }

    fn covered(&self, b: [f64; 4]) -> bool {
        self.rects.iter().any(|r| contains(*r, b, 0.01))
    }

    /// Does a glyph box count as under a region? Any real overlap does (more than a fifth of
    /// the glyph or a point, whichever is less, in both directions), so neighbours that only
    /// touch an edge survive while partly covered glyphs go. Degenerate boxes (zero-size text)
    /// count when their origin is inside.
    fn glyph_hit(&self, b: [f64; 4], origin: (f64, f64)) -> bool {
        let (gw, gh) = (b[2] - b[0], b[3] - b[1]);
        if gw < 0.01 || gh < 0.01 {
            return self.rects.iter().any(|r| origin.0 >= r[0] && origin.0 <= r[2] && origin.1 >= r[1] && origin.1 <= r[3]);
        }
        let (mx, my) = ((gw * 0.2).min(1.0), (gh * 0.2).min(1.0));
        self.rects.iter().any(|r| {
            let ox = b[2].min(r[2]) - b[0].max(r[0]);
            let oy = b[3].min(r[3]) - b[1].max(r[1]);
            ox > mx && oy > my
        })
    }
}

fn res_dict(doc: &Document, res: &Dict, key: &[u8]) -> Dict {
    res.get(key).map(|o| doc.resolve(o)).and_then(|o| o.as_dict().cloned()).unwrap_or_default()
}

fn type_is(doc: &Document, dict: &Dict, name: &[u8]) -> bool {
    name_is(doc, dict, b"Type", name)
}

fn name_is(doc: &Document, dict: &Dict, key: &[u8], name: &[u8]) -> bool {
    dict.get(key).is_some_and(|value| doc.resolve(value).as_name() == Some(name))
}

fn known_advance_font(doc: &Document, dict: &Dict) -> bool {
    // Match this snapshot's Metrics reader: it reads Subtype directly and only
    // implements the predefined two-byte identity CMap for composite fonts.
    let Some(subtype) = dict.name(b"Subtype") else { return false };
    let encoding = dict.get(b"Encoding").map(|value| doc.resolve(value));
    if subtype == b"Type0" {
        return encoding.as_deref().and_then(Object::as_name) == Some(b"Identity-H");
    }
    if !matches!(subtype, b"Type1" | b"MMType1" | b"TrueType") {
        return false;
    }
    match encoding.as_deref() {
        Some(Object::Name(name)) => name != b"V" && !name.ends_with(b"-V"),
        // This snapshot cannot establish an embedded CMap's writing direction here.
        Some(Object::Stream(_)) => false,
        _ => true,
    }
}

fn fresh_name(scope: &mut Scope<'_>, xobjects: &Dict) -> Vec<u8> {
    let mut i = scope.used_names.len() + 1;
    loop {
        let n = format!("PCRedacted{i}").into_bytes();
        if !xobjects.contains(&n) && !scope.used_names.contains(&n) {
            scope.used_names.push(n.clone());
            return n;
        }
        i += 1;
    }
}

/// Adapted from PdfCraft's resource-unlinking dependency at d695bc5. A full save
/// follows resource aliases too, so removed draws must lose their unused aliases.
fn content_draws(doc: &Document, content: &[Vec<u8>], res: &Dict, name: &[u8], depth: usize, seen: &mut HashSet<ObjRef>) -> Option<bool> {
    let parsed = Pieces::join(content).parse();
    if parsed.skipped > 0 || parsed.ops.len() > 2_000_000 {
        return None;
    }
    // These resource users can inherit XObjects without a Do in this stream. The
    // older Strike interpreter cannot establish their usage: refuse cleanup.
    let fonts = res_dict(doc, res, b"Font");
    let type3 = fonts.iter().any(|(_, font)| doc.resolve(font).as_dict().is_some_and(|d| name_is(doc, d, b"Subtype", b"Type3")));
    let text = parsed.ops.iter().any(|op| matches!(op.op.as_slice(), b"Tj" | b"TJ" | b"'" | b"\""));
    let pattern = !res_dict(doc, res, b"Pattern").is_empty() && parsed.ops.iter().any(|op| matches!(op.op.as_slice(), b"scn" | b"SCN"));
    let soft_mask = res_dict(doc, res, b"ExtGState")
        .iter()
        .any(|(_, state)| doc.resolve(state).as_dict().and_then(|d| d.get(b"SMask")).is_some_and(|mask| doc.resolve(mask).as_dict().is_some()))
        && parsed.ops.iter().any(|op| op.is("gs"));
    if (type3 && text) || pattern || soft_mask {
        return None;
    }
    let xo = res_dict(doc, res, b"XObject");
    for op in parsed.ops.iter().filter(|op| op.is("Do")) {
        let n = op.name(0)?;
        if n == name {
            return Some(true);
        }
        let Some(r) = xo.get(n).and_then(Object::as_ref) else { continue };
        let obj = doc.get(r);
        let Object::Stream(s) = &*obj else { continue };
        if !name_is(doc, &s.dict, b"Subtype", b"Form")
            || s.dict.get(b"Resources").is_some_and(|r| doc.resolve(r).as_dict().is_some())
            || !seen.insert(r)
        {
            continue;
        }
        if depth >= MAX_DEPTH || seen.len() > 100_000 {
            return None;
        }
        let data = s.decoded().ok()?;
        match content_draws(doc, &[data], res, name, depth + 1, seen) {
            Some(false) => {}
            other => return other,
        }
    }
    Some(false)
}

/// Structural aliases outside the supported XObject resource paths must not turn
/// an incomplete cleanup into success. Inspect dictionaries, arrays and stream
/// dictionaries without interpreting their payloads or deleting unknown resources.
fn references_doomed(
    doc: &Document,
    value: &Object,
    doomed: &HashSet<ObjRef>,
    path: &mut HashSet<ObjRef>,
    depth: usize,
    budget: &mut usize,
) -> Result<bool, RedactError> {
    if depth > 64 || *budget == 0 {
        return Err(RedactError::Residue(1));
    }
    *budget -= 1;
    match value {
        Object::Ref(reference) => {
            if doomed.contains(reference) {
                return Ok(true);
            }
            if !path.insert(*reference) {
                return Err(RedactError::Residue(1));
            }
            let result = references_doomed(doc, &doc.get(*reference), doomed, path, depth + 1, budget);
            path.remove(reference);
            result
        }
        Object::Array(items) => {
            for item in items {
                if references_doomed(doc, item, doomed, path, depth + 1, budget)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Object::Dict(dict) => dict_references_doomed(doc, dict, doomed, path, depth, budget),
        Object::Stream(stream) => dict_references_doomed(doc, &stream.dict, doomed, path, depth, budget),
        _ => Ok(false),
    }
}

fn dict_references_doomed(
    doc: &Document,
    dict: &Dict,
    doomed: &HashSet<ObjRef>,
    path: &mut HashSet<ObjRef>,
    depth: usize,
    budget: &mut usize,
) -> Result<bool, RedactError> {
    for (_, value) in dict.iter() {
        if references_doomed(doc, value, doomed, path, depth + 1, budget)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn reject_unsupported_aliases(doc: &Document, res: &Dict, doomed: &HashSet<ObjRef>) -> Result<(), RedactError> {
    let mut budget = 100_000;
    let mut path = HashSet::new();
    for (key, value) in res.iter() {
        let supported_xobjects = key == b"XObject" && matches!(&*doc.resolve(value), Object::Dict(_));
        if !supported_xobjects && references_doomed(doc, value, doomed, &mut path, 0, &mut budget)? {
            return Err(RedactError::Residue(1));
        }
    }
    for (_, value) in res_dict(doc, res, b"XObject").iter() {
        // A supported original still drawn outside a mark may remain intentionally.
        if value.as_ref().is_some_and(|r| doomed.contains(&r)) {
            continue;
        }
        // Only stream XObjects enter the supported recursive cleanup. Inspect
        // malformed containers (including reference chains) without exemptions.
        let object = value.as_ref().map_or_else(|| doc.resolve(value), |reference| doc.get(reference));
        let Object::Stream(stream) = &*object else {
            if references_doomed(doc, value, doomed, &mut path, 0, &mut budget)? {
                return Err(RedactError::Residue(1));
            }
            continue;
        };
        let dict = &stream.dict;
        let form = value.as_ref().is_some() && name_is(doc, dict, b"Subtype", b"Form");
        for (key, value) in dict.iter() {
            // Form-owned resources are handled recursively by unlink_form_aliases.
            if form && key == b"Resources" && matches!(&*doc.resolve(value), Object::Dict(_)) {
                continue;
            }
            if references_doomed(doc, value, doomed, &mut path, 0, &mut budget)? {
                return Err(RedactError::Residue(1));
            }
        }
    }
    Ok(())
}

pub(crate) fn prune_xobjects(doc: &Document, res: &mut Dict, doomed: &[ObjRef], contents: &[Vec<u8>]) -> Result<(), RedactError> {
    if doomed.is_empty() {
        return Ok(());
    }
    let doomed: HashSet<ObjRef> = doomed.iter().copied().collect();
    reject_unsupported_aliases(doc, res, &doomed)?;
    let mut xo = res_dict(doc, res, b"XObject");
    let names: Vec<Vec<u8>> = xo.iter().filter(|(_, v)| v.as_ref().is_some_and(|r| doomed.contains(&r))).map(|(n, _)| n.to_vec()).collect();
    for name in names {
        match content_draws(doc, contents, res, &name, 0, &mut HashSet::new()) {
            Some(true) => {}
            Some(false) => {
                xo.remove(&name);
            }
            None => return Err(RedactError::Residue(1)),
        }
    }
    res.set(b"XObject".to_vec(), Object::Dict(xo));
    Ok(())
}

/// Find Forms that can keep a replaced object structurally reachable.
fn affected_forms(doc: &Document, roots: &[ObjRef], doomed: &[ObjRef]) -> Result<HashSet<ObjRef>, RedactError> {
    let mut pending: Vec<Dict> = roots
        .iter()
        .filter_map(|r| doc.get(*r).as_dict().and_then(|d| d.get(b"Resources")).and_then(|r| doc.resolve(r).as_dict().cloned()))
        .collect();
    let mut seen = HashSet::new();
    let mut parents: HashMap<ObjRef, Vec<ObjRef>> = HashMap::new();
    while let Some(resources) = pending.pop() {
        for (_, value) in res_dict(doc, &resources, b"XObject").iter() {
            let Some(reference) = value.as_ref() else { continue };
            if !seen.insert(reference) {
                continue;
            }
            if seen.len() > 100_000 {
                return Err(RedactError::Residue(1));
            }
            let object = doc.get(reference);
            let Object::Stream(form) = &*object else { continue };
            if !name_is(doc, &form.dict, b"Subtype", b"Form") {
                continue;
            }
            let Some(res) = form.dict.get(b"Resources").and_then(|r| doc.resolve(r).as_dict().cloned()) else { continue };
            for (_, child) in res_dict(doc, &res, b"XObject").iter() {
                if let Some(child) = child.as_ref() {
                    parents.entry(child).or_default().push(reference);
                }
            }
            pending.push(res);
        }
    }
    // Reverse reachability includes unused Forms that would otherwise keep an
    // original reachable indirectly through their own resource dictionary.
    let mut affected: HashSet<ObjRef> = doomed.iter().copied().collect();
    let mut pending = doomed.to_vec();
    while let Some(child) = pending.pop() {
        for parent in parents.get(&child).into_iter().flatten() {
            if affected.insert(*parent) {
                pending.push(*parent);
            }
        }
    }
    Ok(affected)
}

fn unlink_form_aliases(doc: &mut Document, roots: &[ObjRef], affected: &HashSet<ObjRef>) -> Result<(), RedactError> {
    let mut pending: Vec<Dict> = roots
        .iter()
        .filter_map(|r| doc.get(*r).as_dict().and_then(|d| d.get(b"Resources")).and_then(|r| doc.resolve(r).as_dict().cloned()))
        .collect();
    let mut seen = HashSet::new();
    while let Some(resources) = pending.pop() {
        for (_, value) in res_dict(doc, &resources, b"XObject").iter() {
            let Some(reference) = value.as_ref() else { continue };
            if !seen.insert(reference) {
                continue;
            }
            if seen.len() > 100_000 {
                return Err(RedactError::Residue(1));
            }
            let object = doc.get(reference);
            let Object::Stream(form) = &*object else { continue };
            if !name_is(doc, &form.dict, b"Subtype", b"Form") {
                continue;
            }
            let Some(mut res) = form.dict.get(b"Resources").and_then(|r| doc.resolve(r).as_dict().cloned()) else { continue };
            reject_unsupported_aliases(doc, &res, affected)?;
            let candidates: Vec<ObjRef> =
                res_dict(doc, &res, b"XObject").iter().filter_map(|(_, value)| value.as_ref()).filter(|r| affected.contains(r)).collect();
            if !candidates.is_empty() {
                prune_xobjects(doc, &mut res, &candidates, &[form.decoded()?])?;
                doc.update_dict(reference, |d| d.set(b"Resources".to_vec(), Object::Dict(res.clone())))?;
            }
            pending.push(res);
        }
    }
    Ok(())
}

pub(crate) fn unlink_doomed(doc: &mut Document, doomed: &[ObjRef]) -> Result<(), RedactError> {
    if doomed.is_empty() {
        return Ok(());
    }
    let Some(top) = doc.root().and_then(|root| doc.get(root).as_dict().and_then(|d| d.reference(b"Pages"))) else { return Ok(()) };
    let mut holders: Vec<(ObjRef, Vec<ObjRef>)> = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![(top, None)];
    while let Some((node, mut holder)) = stack.pop() {
        if !seen.insert(node) {
            continue;
        }
        if seen.len() > 1_000_000 {
            return Err(RedactError::Residue(1));
        }
        let obj = doc.get(node);
        let Some(d) = obj.as_dict() else { continue };
        if d.contains(b"Resources") {
            holders.push((node, Vec::new()));
            holder = Some(holders.len() - 1);
        }
        let is_pages = d.name(b"Type") == Some(b"Pages") || (d.contains(b"Kids") && d.name(b"Type") != Some(b"Page"));
        if is_pages {
            let kids = d.get(b"Kids").map(|k| doc.resolve(k)).and_then(|k| k.as_array().cloned()).unwrap_or_default();
            stack.extend(kids.iter().filter_map(Object::as_ref).map(|k| (k, holder)));
        } else if let Some(h) = holder.and_then(|i| holders.get_mut(i)) {
            h.1.push(node);
        }
    }
    let roots: Vec<ObjRef> = holders.iter().map(|(node, _)| *node).collect();
    let affected = affected_forms(doc, &roots, doomed)?;
    for (node, users) in holders {
        let Some(value) = doc.get(node).as_dict().and_then(|d| d.get(b"Resources")).cloned() else { continue };
        let object = doc.resolve(&value);
        let Object::Dict(resources) = &*object else {
            if references_doomed(doc, &value, &affected, &mut HashSet::new(), 0, &mut 100_000)? {
                return Err(RedactError::Residue(1));
            }
            continue;
        };
        let mut res = resources.clone();
        reject_unsupported_aliases(doc, &res, &affected)?;
        let xo = res_dict(doc, &res, b"XObject");
        let names: Vec<Vec<u8>> = xo.iter().filter(|(_, v)| v.as_ref().is_some_and(|r| affected.contains(&r))).map(|(n, _)| n.to_vec()).collect();
        if names.is_empty() {
            continue;
        }
        let mut contents = Vec::new();
        for user in users {
            let page = doc.get(user).as_dict().cloned().unwrap_or_default();
            contents.push(crate::page_streams(doc, &page, 0)?.1);
        }
        let mut cleaned = xo;
        for name in names {
            let mut used = false;
            for content in &contents {
                match content_draws(doc, content, &res, &name, 0, &mut HashSet::new()) {
                    Some(true) => {
                        used = true;
                        break;
                    }
                    Some(false) => {}
                    None => return Err(RedactError::Residue(1)),
                }
            }
            if !used {
                cleaned.remove(&name);
            }
        }
        res.set(b"XObject".to_vec(), Object::Dict(cleaned));
        doc.update_dict(node, |d| d.set(b"Resources".to_vec(), Object::Dict(res)))?;
    }
    unlink_form_aliases(doc, &roots, &affected)
}

/// The clip that hides everything inside each region: one even-odd clip per region (a big
/// rectangle with the region cut out), in the current user space. Returns `None` when the CTM
/// can't be inverted (nothing is painted then anyway).
fn clip_out(rects: &[[f64; 4]], ctm: &Matrix, around: [f64; 4]) -> Option<Vec<Op>> {
    let inv = ctm.invert()?;
    let mut ops = Vec::new();
    let big = [around[0] - 10.0, around[1] - 10.0, around[2] + 10.0, around[3] + 10.0];
    for r in rects.iter().filter(|r| overlaps(**r, around, 0.0)) {
        let outer = [(big[0], big[1]), (big[2], big[1]), (big[2], big[3]), (big[0], big[3])];
        let inner = [(r[0], r[1]), (r[0], r[3]), (r[2], r[3]), (r[2], r[1])];
        for poly in [outer, inner] {
            for (k, (x, y)) in poly.iter().enumerate() {
                let (u, v) = inv.apply(*x, *y);
                ops.push(Op::new(if k == 0 { "m" } else { "l" }, vec![num(u), num(v)]));
            }
            ops.push(Op::new("h", vec![]));
        }
        ops.push(Op::new("W*", vec![]));
        ops.push(Op::new("n", vec![]));
    }
    Some(ops)
}

/// One glyph of a shown string.
struct Glyph {
    bytes: std::ops::Range<usize>,
    /// Advance in unscaled text space (before `Th`).
    advance: f64,
    hit: bool,
}

/// Process a scope's content streams (state flows from one stream to the next).
pub(crate) fn process(doc: &mut Document, scope: &mut Scope<'_>, streams: &[Vec<u8>], resources: &Dict, ctm: Matrix) -> Output {
    let mut out = Output::default();
    let fallback = Rc::new(Metrics::fallback());
    let mut gs = Gs {
        ctm,
        font: fallback.clone(),
        font_known: false,
        size: 0.0,
        char_spacing: 0.0,
        word_spacing: 0.0,
        render_mode: 0,
        scale: 1.0,
        leading: 0.0,
        rise: 0.0,
    };
    let mut stack: Vec<Gs> = Vec::new();
    let (mut tm, mut tlm) = (Matrix::IDENTITY, Matrix::IDENTITY);
    let fonts_res = res_dict(doc, resources, b"Font");
    let mut xobjects = res_dict(doc, resources, b"XObject");
    // The current path: its operators, its bounding box in user space, and whether it clips.
    let mut path: Vec<Op> = Vec::new();
    let mut path_box: Option<[f64; 4]> = None;
    let mut clip = false;
    let mut forced_change = false;

    // The streams are one content stream in pieces, which may be split between any two tokens
    // (ISO 32000-2 §7.8.2): an operator's operands can end one piece and the operator start the
    // next. Parse them joined, then give each operator back to the piece its keyword is in.
    let joined = Pieces::join(streams);
    let parsed = joined.parse();
    let flagged = if scope.layers_enabled {
        let props = res_dict(doc, resources, b"Properties");
        let (kept, removed) = strip_hidden_layers(doc, scope, parsed.ops, &props);
        if removed > 0 {
            scope.layer_blocks += removed;
            forced_change = true;
        }
        kept
    } else {
        parsed.ops.into_iter().map(|op| (op, false)).collect()
    };
    let mut pieces: Vec<Vec<(Op, bool)>> = streams.iter().map(|_| Vec::new()).collect();
    let mut rewrite: Vec<bool> = vec![forced_change; streams.len()];
    for (op, hidden) in flagged {
        let (first, last) = joined.pieces_of(&op);
        // An operator split across pieces is written whole into its keyword's piece, so every
        // piece it spans is rewritten.
        if first < last {
            for r in rewrite.iter_mut().take(last + 1).skip(first) {
                *r = true;
            }
        }
        if let Some(p) = pieces.get_mut(last) {
            p.push((op, hidden));
        }
    }

    for (piece, mut changed) in pieces.into_iter().zip(rewrite) {
        let mut ops: Vec<Op> = Vec::with_capacity(piece.len());
        for (op, hidden) in piece {
            let o = op.op.as_slice();
            // Path construction.
            if matches!(o, b"m" | b"l" | b"c" | b"v" | b"y" | b"h" | b"re" | b"W" | b"W*") {
                let pts: Vec<(f64, f64)> = match o {
                    b"re" => op.nums::<4>().map(|[x, y, w, h]| vec![(x, y), (x + w, y), (x, y + h), (x + w, y + h)]).unwrap_or_default(),
                    b"h" | b"W" | b"W*" => Vec::new(),
                    _ => op.operands.iter().filter_map(Object::as_f64).collect::<Vec<_>>().as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).collect(),
                };
                for (x, y) in pts {
                    let (u, v) = gs.ctm.apply(x, y);
                    path_box = Some(match path_box {
                        None => [u, v, u, v],
                        Some(b) => [b[0].min(u), b[1].min(v), b[2].max(u), b[3].max(v)],
                    });
                }
                clip |= matches!(o, b"W" | b"W*");
                path.push(op);
                continue;
            }
            if matches!(o, b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n") {
                let body = std::mem::take(&mut path);
                let bbox = path_box.take();
                let clips = std::mem::replace(&mut clip, false);
                if hidden {
                    changed = true;
                    // Clipping remains state even when its associated painting is hidden.
                    if clips {
                        ops.extend(body);
                    }
                    ops.push(Op::new("n", vec![]));
                    continue;
                }
                let hit = scope.mode == Mode::Apply && o != b"n" && bbox.is_some_and(|b| scope.hits(b));
                if !hit {
                    ops.extend(body);
                    ops.push(op);
                    continue;
                }
                let b = bbox.unwrap_or_default();
                changed = true;
                if scope.covered(b) {
                    scope.report.paths_removed += 1;
                    if clips {
                        // Keep the clipping, drop the painting.
                        ops.extend(body);
                        ops.push(Op::new("n", vec![]));
                    }
                } else if clips {
                    // A clip-and-paint path can't be wrapped in q/Q without losing its clip:
                    // paint it clipped, then set its clip with a no-op path.
                    scope.report.paths_clipped += 1;
                    if let Some(c) = clip_out(scope.rects, &gs.ctm, b) {
                        let plain: Vec<Op> = body.iter().filter(|p| !p.is("W") && !p.is("W*")).cloned().collect();
                        ops.push(Op::new("q", vec![]));
                        ops.extend(c);
                        ops.extend(plain);
                        ops.push(op);
                        ops.push(Op::new("Q", vec![]));
                    }
                    ops.extend(body);
                    ops.push(Op::new("n", vec![]));
                } else {
                    scope.report.paths_clipped += 1;
                    if let Some(c) = clip_out(scope.rects, &gs.ctm, b) {
                        ops.push(Op::new("q", vec![]));
                        ops.extend(c);
                        ops.extend(body);
                        ops.push(op);
                        ops.push(Op::new("Q", vec![]));
                    }
                }
                continue;
            }
            if !path.is_empty() {
                // A path not ended by a painting operator: pass it through.
                ops.append(&mut path);
                path_box = None;
                clip = false;
            }
            match o {
                b"q" => stack.push(gs.clone()),
                b"Q" => {
                    if let Some(g) = stack.pop() {
                        gs = g;
                    }
                }
                b"cm" => {
                    if let Some(m) = op.nums::<6>() {
                        gs.ctm = Matrix(m).then(&gs.ctm);
                    }
                }
                b"BT" => {
                    tm = Matrix::IDENTITY;
                    tlm = Matrix::IDENTITY;
                }
                b"Tf" => {
                    gs.size = op.num(1).unwrap_or(gs.size);
                    if let Some(name) = op.name(0) {
                        gs.font_known = fonts_res
                            .get(name)
                            .and_then(|font| doc.resolve(font).as_dict().cloned())
                            .is_some_and(|dict| known_advance_font(doc, &dict));
                        gs.font = match scope.fonts.get(name) {
                            Some(f) => f.clone(),
                            None => {
                                let m = fonts_res
                                    .get(name)
                                    .and_then(|f| doc.resolve(f).as_dict().cloned())
                                    .map(|d| Rc::new(Metrics::from_dict(doc, &d)))
                                    .unwrap_or_else(|| fallback.clone());
                                scope.fonts.insert(name.to_vec(), m.clone());
                                m
                            }
                        };
                    }
                }
                b"gs" => {
                    let states = res_dict(doc, resources, b"ExtGState");
                    if let Some(state) = op.name(0).and_then(|name| states.get(name)).map(|value| doc.resolve(value))
                        && let Some(font) = state.as_dict().and_then(|dict| dict.get(b"Font"))
                    {
                        let font = doc.resolve(font);
                        let pair = font.as_array().filter(|pair| pair.len() == 2);
                        let dict = pair.and_then(|pair| doc.resolve(&pair[0]).as_dict().cloned());
                        let size = pair.and_then(|pair| doc.resolve(&pair[1]).as_f64()).filter(|size| size.is_finite());
                        gs.font_known = false;
                        if let (Some(dict), Some(size)) = (dict, size) {
                            gs.font = Rc::new(Metrics::from_dict(doc, &dict));
                            gs.font_known = known_advance_font(doc, &dict);
                            gs.size = size;
                        }
                    }
                }
                b"Tc" => gs.char_spacing = op.num(0).unwrap_or(0.0),
                b"Tw" => gs.word_spacing = op.num(0).unwrap_or(0.0),
                b"Tz" => gs.scale = op.num(0).unwrap_or(100.0) / 100.0,
                b"TL" => gs.leading = op.num(0).unwrap_or(0.0),
                b"Ts" => gs.rise = op.num(0).unwrap_or(0.0),
                b"Tr" => gs.render_mode = op.num(0).unwrap_or(0.0) as i64,
                b"Td" | b"TD" => {
                    if let Some([x, y]) = op.nums::<2>() {
                        if o == b"TD" {
                            gs.leading = -y;
                        }
                        tlm = Matrix::translate(x, y).then(&tlm);
                        tm = tlm;
                    }
                }
                b"Tm" => {
                    if let Some(m) = op.nums::<6>() {
                        tlm = Matrix(m);
                        tm = tlm;
                    }
                }
                b"T*" => {
                    tlm = Matrix::translate(0.0, -gs.leading).then(&tlm);
                    tm = tlm;
                }
                b"Tj" | b"TJ" | b"'" | b"\"" => {
                    if o == b"\"" {
                        gs.word_spacing = op.num(0).unwrap_or(gs.word_spacing);
                        gs.char_spacing = op.num(1).unwrap_or(gs.char_spacing);
                    }
                    if matches!(o, b"'" | b"\"") {
                        tlm = Matrix::translate(0.0, -gs.leading).then(&tlm);
                        tm = tlm;
                    }
                    let items: Vec<Object> = match o {
                        b"TJ" => op.operands.first().and_then(Object::as_array).cloned().unwrap_or_default(),
                        _ => op.operands.last().cloned().into_iter().collect(),
                    };
                    // Removing clip-text would change later visible content. This interpreter
                    // cannot replace its glyph outlines without retaining the secret string.
                    if hidden
                        && scope.mode == Mode::Apply
                        && (!gs.font_known
                            || (4..=7).contains(&gs.render_mode)
                            || (gs.size == 0.0 && (gs.char_spacing != 0.0 || gs.word_spacing != 0.0)))
                    {
                        out.error = Some(RedactError::Residue(1));
                        return out;
                    }
                    let (new_items, removed) = show(scope, &gs, &mut tm, &items, hidden);
                    if removed == 0 {
                        ops.push(op);
                        continue;
                    }
                    if scope.mode == Mode::Verify {
                        if !hidden {
                            out.residue += removed;
                        }
                        ops.push(op);
                        continue;
                    }
                    changed = true;
                    if !hidden {
                        scope.report.glyphs += removed;
                    }
                    match o {
                        b"\"" => {
                            ops.push(Op::new("Tw", vec![num(gs.word_spacing)]));
                            ops.push(Op::new("Tc", vec![num(gs.char_spacing)]));
                            ops.push(Op::new("T*", vec![]));
                        }
                        b"'" => ops.push(Op::new("T*", vec![])),
                        _ => {}
                    }
                    ops.push(Op::new("TJ", vec![Object::Array(new_items)]));
                    continue;
                }
                b"BI" => {
                    if hidden {
                        changed = true;
                        continue;
                    }
                    let b = gs.ctm.bbox([0.0, 0.0, 1.0, 1.0]);
                    if scope.hits(b) {
                        if scope.mode == Mode::Verify {
                            out.residue += 1;
                        } else {
                            scope.report.images_removed += 1;
                            changed = true;
                            continue;
                        }
                    }
                }
                b"sh" if scope.mode == Mode::Apply => {
                    if hidden {
                        changed = true;
                        continue;
                    }
                    // A shading fills the current clip: clip the regions out.
                    if let Some(c) = clip_out(scope.rects, &gs.ctm, [-1e6, -1e6, 1e6, 1e6]) {
                        changed = true;
                        ops.push(Op::new("q", vec![]));
                        ops.extend(c);
                        ops.push(op);
                        ops.push(Op::new("Q", vec![]));
                        continue;
                    }
                }
                b"Do" => {
                    if hidden {
                        if scope.mode == Mode::Apply {
                            if let Some(reference) = op.name(0).and_then(|name| xobjects.get(name)).and_then(Object::as_ref) {
                                out.doomed.push(reference);
                            }
                            changed = true;
                        }
                        continue;
                    }
                    if let Some(new) = xobject(doc, scope, &op, &gs, resources, &mut xobjects, &mut out) {
                        changed = true;
                        if let Some(n) = new {
                            ops.push(n);
                        }
                        continue;
                    }
                }
                _ => {}
            }
            // Preserve persistent state, while dropping hidden marked points, unknown
            // operators, and their payloads. Text and painting are handled above.
            if !hidden
                || matches!(
                    o,
                    b"q" | b"Q"
                        | b"cm"
                        | b"w"
                        | b"J"
                        | b"j"
                        | b"M"
                        | b"d"
                        | b"ri"
                        | b"i"
                        | b"gs"
                        | b"CS"
                        | b"cs"
                        | b"SC"
                        | b"SCN"
                        | b"sc"
                        | b"scn"
                        | b"G"
                        | b"g"
                        | b"RG"
                        | b"rg"
                        | b"K"
                        | b"k"
                        | b"BT"
                        | b"ET"
                        | b"Tf"
                        | b"Tc"
                        | b"Tw"
                        | b"Tz"
                        | b"TL"
                        | b"Ts"
                        | b"Tr"
                        | b"Td"
                        | b"TD"
                        | b"Tm"
                        | b"T*"
                        | b"BX"
                        | b"EX"
                )
            {
                ops.push(op);
            }
        }
        if !path.is_empty() {
            ops.append(&mut path);
        }
        out.streams.push(changed.then(|| serialize_ops(&ops)));
    }
    out
}

/// Lay out a shown string (or `TJ` array) glyph by glyph, advancing `tm`. Returns the `TJ`
/// items with the glyphs under a region replaced by displacements, and how many were removed.
fn show(scope: &Scope<'_>, gs: &Gs, tm: &mut Matrix, items: &[Object], hidden: bool) -> (Vec<Object>, usize) {
    let f = &gs.font;
    let size = gs.size;
    let mut out: Vec<Object> = Vec::new();
    let mut removed = 0;
    let push_num = |out: &mut Vec<Object>, v: f64| {
        if v == 0.0 {
            return;
        }
        if let Some(last) = out.last_mut()
            && let Some(prev) = last.as_f64()
        {
            *last = num(prev + v);
            return;
        }
        out.push(num(v));
    };
    for item in items {
        match item {
            Object::String(s) => {
                let bytes = &s.bytes;
                let mut glyphs = Vec::new();
                let mut pos = 0;
                for (code, len) in f.codes(bytes) {
                    let w0 = f.width(code);
                    let spacing = gs.char_spacing + if f.is_space(code, len) { gs.word_spacing } else { 0.0 };
                    let trm = Matrix([size * gs.scale, 0.0, 0.0, size, 0.0, gs.rise]).then(tm).then(&gs.ctm);
                    let b = trm.bbox([0.0, f.descent, w0, f.ascent]);
                    let origin = trm.apply(0.0, 0.0);
                    let advance = w0 * size + spacing;
                    let hit = hidden
                        || match scope.hidden_text {
                            // Invisible (Tr 3) and clip-only (Tr 7) text, or text wholly off the page.
                            Some(page) => {
                                let degenerate = b[2] - b[0] < 0.01 && b[3] - b[1] < 0.01;
                                matches!(gs.render_mode, 3 | 7) || !(overlaps(page, b, 0.0) || degenerate)
                            }
                            None => scope.glyph_hit(b, origin),
                        };
                    glyphs.push(Glyph { bytes: pos..pos + len, advance, hit });
                    *tm = Matrix::translate(advance * gs.scale, 0.0).then(tm);
                    pos += len;
                }
                let mut run: Vec<u8> = Vec::new();
                for g in &glyphs {
                    if g.hit {
                        removed += 1;
                        if !run.is_empty() {
                            out.push(string(std::mem::take(&mut run)));
                        }
                        // A TJ number moves by -n/1000 × size (× Th): the removed advance.
                        if size != 0.0 {
                            push_num(&mut out, -g.advance * 1000.0 / size);
                        }
                    } else {
                        run.extend_from_slice(&bytes[g.bytes.clone()]);
                    }
                }
                if !run.is_empty() {
                    out.push(string(run));
                }
            }
            other => {
                if let Some(v) = other.as_f64() {
                    *tm = Matrix::translate(-v / 1000.0 * size * gs.scale, 0.0).then(tm);
                    push_num(&mut out, v);
                }
            }
        }
    }
    (out, removed)
}

/// `Do`: `None` = leave the operator alone; `Some(None)` = drop it; `Some(Some(op))` = replace it.
fn xobject(
    doc: &mut Document,
    scope: &mut Scope<'_>,
    op: &Op,
    gs: &Gs,
    resources: &Dict,
    xobjects: &mut Dict,
    out: &mut Output,
) -> Option<Option<Op>> {
    let name = op.name(0)?.to_vec();
    let r = xobjects.get(&name)?.as_ref()?;
    let obj = doc.get(r);
    let Object::Stream(s) = &*obj else { return None };
    let subtype = s.dict.get(b"Subtype").map(|value| doc.resolve(value));
    match subtype.as_deref().and_then(Object::as_name) {
        _ if s.dict.get(b"OC").is_some_and(|oc| scope.xobject_layer_hidden(doc, r, oc)) => {
            // Verify mode must count hidden XObjects too: scan uses that count to decide
            // whether the HiddenLayers pass is necessary.
            scope.layer_blocks += 1;
            if scope.mode == Mode::Verify {
                return None;
            }
            out.doomed.push(r);
            Some(None)
        }
        Some(b"Image") => {
            let b = gs.ctm.bbox([0.0, 0.0, 1.0, 1.0]);
            if !scope.hits(b) || scope.mode == Mode::Verify {
                return None;
            }
            if scope.covered(b) {
                scope.report.images_removed += 1;
                out.doomed.push(r);
                return Some(None);
            }
            match image::clear(doc, s, &gs.ctm, scope.rects) {
                Ok(None) => None,
                Ok(Some(new)) => {
                    let nr = doc.add(Object::Stream(new));
                    let n = fresh_name(scope, xobjects);
                    xobjects.set(n.clone(), Object::Ref(nr));
                    out.xobjects.push((n.clone(), nr));
                    out.doomed.push(r);
                    scope.report.images_cleared += 1;
                    Some(Some(Op::new("Do", vec![Object::Name(n)])))
                }
                Err(()) => {
                    scope.report.images_removed += 1;
                    out.doomed.push(r);
                    Some(None)
                }
            }
        }
        Some(b"Form") => {
            let m = s.dict.get(b"Matrix").and_then(|m| Matrix::from_operands(doc.resolve(m).as_array()?)).unwrap_or_default();
            let fctm = m.then(&gs.ctm);
            let bbox = s.dict.get(b"BBox").and_then(|b| {
                let v: Vec<f64> = doc.resolve(b).as_array()?.iter().filter_map(Object::as_f64).collect();
                (v.len() == 4).then(|| [v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
            });
            if let Some(bb) = bbox
                && !scope.everywhere()
                && !scope.hits(fctm.bbox(bb))
            {
                return None;
            }
            if scope.depth >= MAX_DEPTH {
                if scope.mode == Mode::Verify {
                    // Sanitization removes a Form it cannot enter, so the verification scan
                    // must count that conservative removal or it may decide to skip the pass.
                    if scope.everywhere() {
                        scope.layer_blocks += 1;
                    }
                    return None;
                }
                scope.report.forms_removed += 1;
                out.doomed.push(r);
                return Some(None);
            }
            let data = match s.decoded() {
                Ok(d) => d,
                Err(_) if scope.mode == Mode::Apply => {
                    scope.report.forms_removed += 1;
                    out.doomed.push(r);
                    return Some(None);
                }
                Err(_) => {
                    // Apply drops an undecodable Form while sanitizing. Count it during Verify
                    // so a zero-count preflight cannot leave its potentially hidden bytes behind.
                    if scope.everywhere() {
                        scope.layer_blocks += 1;
                    }
                    return None;
                }
            };
            let own = s.dict.get(b"Resources").and_then(|r| doc.resolve(r).as_dict().cloned());
            let res = own.clone().unwrap_or_else(|| resources.clone());
            let saved_fonts = std::mem::take(&mut scope.fonts);
            scope.depth += 1;
            let inner = process(doc, scope, std::slice::from_ref(&data), &res, fctm);
            scope.depth -= 1;
            scope.fonts = saved_fonts;
            out.residue += inner.residue;
            out.doomed.extend_from_slice(&inner.doomed);
            out.error = inner.error.or(out.error.take());
            if scope.mode == Mode::Verify {
                return None;
            }
            let new_data = inner.streams.into_iter().next().flatten();
            if new_data.is_none() && inner.xobjects.is_empty() {
                return None;
            }
            let mut dict = s.dict.clone();
            dict.remove(b"Length");
            let mut cleaned_res = res.clone();
            // Pin the rewritten Form's resources. Otherwise an inherited alias can keep the
            // original enclosing Form (and its removed content) reachable through the copy.
            let mut xo = res_dict(doc, &cleaned_res, b"XObject");
            let candidates: Vec<ObjRef> = xo.iter().filter_map(|(_, value)| value.as_ref()).collect();
            for (name, reference) in &inner.xobjects {
                xo.set(name.clone(), Object::Ref(*reference));
            }
            cleaned_res.set(b"XObject".to_vec(), Object::Dict(xo));
            if let Err(error) = prune_xobjects(doc, &mut cleaned_res, &candidates, &[new_data.clone().unwrap_or_else(|| data.clone())]) {
                out.error = Some(error);
                return None;
            }
            dict.set(b"Resources".to_vec(), Object::Dict(cleaned_res));
            let stream = match new_data {
                Some(d) => Stream::flate(dict, &d),
                None => Stream { dict, raw: s.raw.clone() },
            };
            let nr = doc.add(Object::Stream(stream));
            let n = fresh_name(scope, xobjects);
            xobjects.set(n.clone(), Object::Ref(nr));
            out.xobjects.push((n.clone(), nr));
            out.doomed.push(r);
            scope.report.forms_rewritten += 1;
            Some(Some(Op::new("Do", vec![Object::Name(n)])))
        }
        _ => None,
    }
}

/// Remove hidden layer wrappers and flag their enclosed operators for the interpreter.
/// Painting/payloads are removed there while persistent graphics/text state is preserved.
fn strip_hidden_layers(doc: &Document, scope: &mut Scope<'_>, ops: Vec<Op>, props: &Dict) -> (Vec<(Op, bool)>, usize) {
    let mut out = Vec::with_capacity(ops.len());
    // For each open marked-content block: does it hide its contents?
    let mut stack: Vec<bool> = Vec::new();
    let mut removed = 0;
    for op in ops {
        let hidden_now = stack.last().copied().unwrap_or(false);
        match op.op.as_slice() {
            b"BDC" | b"BMC" => {
                let this = op.is("BDC")
                    && op.name(0) == Some(b"OC")
                    && match op.operands.get(1) {
                        Some(Object::Name(n)) => props.get(n).is_some_and(|o| scope.layer_hidden(doc, o)),
                        Some(o) => scope.layer_hidden(doc, o),
                        None => false,
                    };
                if this && !hidden_now {
                    removed += 1;
                }
                stack.push(hidden_now || this);
                if !(hidden_now || this) {
                    out.push((op, false));
                }
            }
            b"EMC" => {
                let was = stack.pop().unwrap_or(false);
                if !was {
                    out.push((op, false));
                }
            }
            _ => {
                out.push((op, hidden_now));
            }
        }
    }
    (out, removed)
}
