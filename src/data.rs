use crate::{hash::Cuckoo, poly_interpolate::newton_interpolate};
use itertools::{izip, Itertools};
use ndarray::Array2;
use std::ops::Deref;

/// No. of rows on a hash table
#[derive(Clone, Debug)]
pub struct HashTableSize(u32);

impl Deref for HashTableSize {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Debug)]
pub struct PsiPlaintext {
    psi_pt_bits: u32,
    bfv_pt_bits: u32,
    bfv_pt: u32,
}

impl PsiPlaintext {
    fn new(psi_pt_bits: u32, bfv_bt_bits: u32, bfv_pt: u32) -> PsiPlaintext {
        PsiPlaintext {
            psi_pt_bits,
            bfv_pt_bits: bfv_bt_bits,
            bfv_pt,
        }
    }

    fn slots_required(&self) -> u32 {
        (self.psi_pt_bits + (self.bfv_pt_bits >> 1)) / self.bfv_pt_bits
    }

    fn chunk_bits(&self) -> u32 {
        self.bfv_pt_bits
    }
}

/// No. of slots in a single BFV ciphertext. Equivalent to degree of ciphertext.
#[derive(Clone, Debug)]
pub struct CiphertextSlots(u32);

/// Degree of interpolated polynomial
#[derive(Clone, Debug)]
pub struct EvalPolyDegree(u32);

/// Warning: We assume that bits in both label and item are equal.
pub struct ItemLabel(u128, u128);
impl ItemLabel {
    pub fn new(item: u128, label: u128) -> ItemLabel {
        ItemLabel(item, label)
    }

    pub fn item(&self) -> u128 {
        self.0
    }

    pub fn label(&self) -> u128 {
        self.1
    }

    /// `item` is greater
    ///
    /// TODO: Switch this to an iterator
    pub fn get_chunk_at_index(&self, chunk_index: u32, psi_pt: &PsiPlaintext) -> (u32, u32) {
        let bits = psi_pt.chunk_bits();
        let mask = (1 << bits) - 1;

        (
            (self.item() >> ((chunk_index * bits) as u128) & mask) as u32,
            (self.item() >> ((chunk_index * bits) as u128) & mask) as u32,
        )
    }
}

/// A single InnerBoxRow is a wrapper over `span` rows.
/// It helps view a single column spanned across multiple
/// rows as a single row. This is required since a single data
/// entry spans across multiple Rows.
struct InnerBoxRow {
    /// No. of rows in real a single InnerBoxRow spans to
    span: u32,
    eval_degree: EvalPolyDegree,
    // no. of curr columns occupied
    curr_cols: u32,
}
impl InnerBoxRow {
    fn new(span: u32, eval_degree: &EvalPolyDegree) -> InnerBoxRow {
        InnerBoxRow {
            span,
            eval_degree: eval_degree.clone(),
            curr_cols: 0,
        }
    }

    /// A row has columns equivalent to iterpolated polynomial degree
    fn max_cols(&self) -> u32 {
        self.eval_degree.0
    }

    /// Returns boolean indicating whether you can insert data into the row.
    /// A row is considered fully occupied when all its columns are filled.
    fn is_free(&self) -> bool {
        self.curr_cols < self.eval_degree.0
    }

    /// `curr_cols` indicate no. of columns occupied. So the next free index is `curr_cols` value.
    fn next_free_col_index(&self) -> usize {
        self.curr_cols as usize
    }

    fn map_to_real_row(&self, row: usize) -> usize {
        self.span as usize * row
    }
}

pub struct InnerBox {
    coefficients_data: Array2<u32>,
    item_data: Array2<u32>,
    label_data: Array2<u32>,
    ht_rows: Vec<InnerBoxRow>,
    /// Is set to initialised when a new item is added
    initialised: bool,
    psi_pt: PsiPlaintext,
}

impl InnerBox {
    /// Since a single item spans across `lane_span`. InnerBox
    /// has bfv_degree / lane_span hash table rows. Remember that each `HashTableRow`
    /// has `lane_span`rows.
    fn new(
        psi_pt: &PsiPlaintext,
        ct_slots: &CiphertextSlots,
        eval_degree: &EvalPolyDegree,
    ) -> InnerBox {
        // A single entry spans across multiple slots
        let slots_per_entry = psi_pt.slots_required();
        let row_count = ct_slots.0 / slots_per_entry;
        let ht_rows = (0..row_count)
            .into_iter()
            .map(|_| InnerBoxRow::new(slots_per_entry, eval_degree))
            .collect_vec();

        // initialise containers for data
        let label_data = Array2::<u32>::zeros((ct_slots.0 as usize, eval_degree.0 as usize));
        let item_data = Array2::<u32>::zeros((ct_slots.0 as usize, eval_degree.0 as usize));

        println!(
            "Created InnerBox with {row_count} rows and {} cols",
            eval_degree.0
        );

        InnerBox {
            coefficients_data: Array2::zeros((0, 0)),
            item_data,
            label_data,
            ht_rows,
            initialised: false,
            psi_pt: psi_pt.clone(),
        }
    }

    /// Returns whether there's space to insert an ItemLabel in row at `index`.
    fn can_insert(&self, index: usize) -> bool {
        self.ht_rows[index].is_free()
    }

    /// Insert item label at row
    fn insert_item_label(&mut self, row: usize, item_label: &ItemLabel, psi_pt: &PsiPlaintext) {
        // get next free column at InnerRow
        let col = self.ht_rows[row].next_free_col_index();
        // map InnerRow to row in container row
        let real_row = row * self.psi_pt.slots_required() as usize;
        for i in real_row..(real_row + self.psi_pt.slots_required() as usize) {
            // get data chunk
            let chunk_index = (i - real_row) as u32;
            let (item_chunk, label_chunk) = item_label.get_chunk_at_index(chunk_index, psi_pt);

            println!(
                "[IB] Inserting ItemLabel - item:{}, chunk_index:{chunk_index}, chunk:{item_chunk}, InnerBox Row:{row}, Real Row:{i}",
                item_label.item(),
            );

            // add the item and label chunk
            let entry = self.item_data.get_mut((i, col)).unwrap();
            *entry = item_chunk;
            let entry = self.label_data.get_mut((i, col)).unwrap();
            *entry = label_chunk;
        }

        // increase columns occupancy by 1
        self.ht_rows[row].curr_cols += 1;
        self.initialised = true;
    }

    /// Returns maximum no. of rows it can have depending on params
    fn max_rows(psi_pt: &PsiPlaintext, ct_slots: &CiphertextSlots) -> u32 {
        ct_slots.0 / psi_pt.slots_required()
    }

    /// Iterates through all rows and generates coefficients
    ///
    /// TODO: Avoid rows that haven't been touched
    fn generate_coefficients(&mut self) {
        println!(
            "
            [IB] Generating Coefficients
                    InnerBoxRows: {},
                    No. of polynomials to interpolate: {} each with degree: {},
        ",
            self.ht_rows.len(),
            self.item_data.shape()[0],
            self.item_data.shape()[1]
        );
        let shape = self.item_data.shape();
        self.coefficients_data = Array2::<u32>::zeros((shape[0], shape[1]));
        izip!(
            self.coefficients_data.outer_iter_mut(),
            self.item_data.outer_iter(),
            self.label_data.outer_iter()
        )
        .for_each(|(mut coeffs, item, label)| {
            let c = newton_interpolate(
                item.as_slice().unwrap(),
                label.as_slice().unwrap(),
                self.psi_pt.bfv_pt as u32,
            );
            coeffs.as_slice_mut().unwrap().copy_from_slice(&c);
        });
    }
}

/// BigBox contains 2D array of InnerBoxes. BigBox has as many as HashTableSize rows. It divides its rows
/// into multiple segments (HashTableSiz/InnerBox::rows) and assign a vec of InnerBoxes to each segment. You must view
/// the row at which ItemLabel is inserted as the row of InnerBoxes corresponding to segment into which the row falls.
/// ItemLabel is always appened at the end of the row. To insert, it is checked whether the last InnerBox corresponding in vec corresponding
/// to segment has enough space at row. If yes, then ItemLabel is inserted. Otherwise, a new InnerBox is created and appended to vec and then
/// the item is inserted.
pub struct BigBox {
    /// Although inner_boxes is a 2d array of `InnerBox`, can't store it as such since length of each row is a not equal
    inner_boxes: Vec<Vec<InnerBox>>,
    ht_size: HashTableSize,
    ct_slots: CiphertextSlots,
    eval_degree: EvalPolyDegree,
    psi_pt: PsiPlaintext,
    inner_box_rows: u32,
}

impl BigBox {
    pub fn new(
        ht_size: &HashTableSize,
        ct_slots: &CiphertextSlots,
        eval_degree: &EvalPolyDegree,
        psi_pt: &PsiPlaintext,
    ) -> BigBox {
        // rows in single inner box

        let inner_box_rows = InnerBox::max_rows(psi_pt, ct_slots);

        let segments = (ht_size.0 + (inner_box_rows >> 1)) / inner_box_rows;
        let mut inner_boxes = vec![];
        // setup inner boxes for stack rows
        (0..segments)
            .into_iter()
            .for_each(|_| inner_boxes.push(vec![InnerBox::new(psi_pt, ct_slots, eval_degree)]));

        BigBox {
            inner_boxes,
            ht_size: ht_size.clone(),
            ct_slots: ct_slots.clone(),
            eval_degree: eval_degree.clone(),
            psi_pt: psi_pt.clone(),
            inner_box_rows,
        }
    }

    /// Returns the segment in which `ht_index` falls
    fn ht_index_to_segment_index(&self, ht_index: usize) -> usize {
        ht_index / self.inner_box_rows as usize
    }

    // Maps ht_index to row of InnerBox in a segment
    fn ht_index_to_inner_box_row(&self, ht_index: usize) -> usize {
        ht_index % self.inner_box_rows as usize
    }

    pub fn insert(&mut self, item_label: &ItemLabel, ht_index: usize) {
        let segment_index = self.ht_index_to_segment_index(ht_index);
        let inner_box_row = self.ht_index_to_inner_box_row(ht_index);

        println!(
            "[BB] Inserting item: {} at ht_index: {}; segment_index: {}, ib_row: {}",
            item_label.label(),
            ht_index,
            segment_index,
            inner_box_row
        );

        // Find the first InnerBox in segment that has free space at row
        let mut inner_box_index = None;
        for i in 0..self.inner_boxes[segment_index].len() {
            if self.inner_boxes[segment_index][i].can_insert(inner_box_row) {
                inner_box_index = Some(i);
                break;
            }
        }
        if inner_box_index.is_none() {
            println!(
                "[BB] All InnerBoxes at sgement {segment_index} at row {inner_box_row} are full. Creating new IB"
            );
            // None of the inner boxes in segment have space available at row. Create a new one.
            self.inner_boxes[segment_index].push(InnerBox::new(
                &self.psi_pt,
                &self.ct_slots,
                &self.eval_degree,
            ));
            // set the index to newly inserted InnerBox
            inner_box_index = Some(self.inner_boxes[segment_index].len() - 1);
        }
        let inner_box_index = inner_box_index.unwrap();

        // insert item label
        self.inner_boxes[segment_index][inner_box_index].insert_item_label(
            inner_box_row,
            item_label,
            &self.psi_pt,
        );

        println!(
            "[BB] Item {} for ht_index:{ht_index} inserted; segment {segment_index}, inner_box_index {inner_box_index}, ib_row: {inner_box_row}",
            item_label.item()
        );
    }

    /// Proprocesses each InnerBox
    pub fn preprocess(&mut self) {
        self.inner_boxes
            .iter_mut()
            .enumerate()
            .for_each(|(s_i, segment)| {
                segment.iter_mut().enumerate().for_each(|(i, ib)| {
                    println!("[BB] Preprocessing IB from segment {s_i} at index {i}");
                    ib.generate_coefficients();
                });
            });
    }
}

struct Db {
    cuckoo: Cuckoo,
    big_boxes: Vec<BigBox>,
}

impl Db {
    pub fn new(
        no_of_hash_tables: u8,
        ht_size: &HashTableSize,
        ct_slots: &CiphertextSlots,
        eval_degree: &EvalPolyDegree,
        psi_pt: &PsiPlaintext,
    ) -> Db {
        let cuckoo = Cuckoo::new(no_of_hash_tables, **ht_size);
        let big_boxes = (0..no_of_hash_tables)
            .into_iter()
            .map(|i| BigBox::new(ht_size, ct_slots, eval_degree, psi_pt))
            .collect_vec();

        Db { cuckoo, big_boxes }
    }

    pub fn insert(&mut self, item: u128, label: u128) {
        // get index for item for all hash tables
        let indices = self.cuckoo.table_indices(item);

        let item_label = ItemLabel::new(item, label);
        // insert item at index corresponding to hash table
        izip!(self.big_boxes.iter_mut(), indices.iter()).for_each(|(big_box, ht_index)| {
            big_box.insert(&item_label, *ht_index as usize);
        })
    }

    pub fn preprocess(&mut self) {
        self.big_boxes.iter_mut().for_each(|bb| bb.preprocess());
    }
}

#[cfg(test)]
mod tests {
    use rand::{thread_rng, Rng};

    use super::*;

    #[test]
    fn db_works() {
        let ht_size = HashTableSize(4096);
        let ct_slots = CiphertextSlots(8192);
        let eval_degree = EvalPolyDegree(2000);
        let psi_pt = PsiPlaintext::new(128, 16, 65537);
        let mut db = Db::new(3, &ht_size, &ct_slots, &eval_degree, &psi_pt);

        println!(
            "
            slot_span: {},
        ",
            psi_pt.slots_required()
        );

        let mut rng = thread_rng();
        for i in 0..10 {
            let item: u128 = rng.gen();
            let label: u128 = rng.gen();

            db.insert(item, label);
        }

        db.preprocess();
    }
}
