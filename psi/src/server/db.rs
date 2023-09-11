use rand_chacha::rand_core::le;
use rayon::{prelude::*, slice::ParallelSlice};

use super::*;

/// Vector of `HashTableQueryResponse`, one for each BigBox
#[derive(Debug, PartialEq)]
pub struct QueryResponse(pub(crate) Vec<HashTableQueryResponse>);

/// Contains 2D array of ciphertexts where each row contains response ciphertexts corresponding to a single Segment in BigBox (ie hash table)
#[derive(Debug, PartialEq)]
pub struct HashTableQueryResponse(pub(crate) Vec<Vec<Ciphertext>>);

/// A single InnerBoxRow is a wrapper over `span` rows.
/// It helps view a single column spanned across multiple
/// rows as a single row. This is required since a single data
/// entry spans across multiple Rows.
pub struct InnerBoxRow {
    /// No. of rows in real a single InnerBoxRow spans to
    span: u32,
    max_cols: u32,
    // no. of curr columns occupied
    curr_cols: u32,
}
impl InnerBoxRow {
    fn new(span: u32, eval_degree: &EvalPolyDegree) -> InnerBoxRow {
        InnerBoxRow {
            span,
            max_cols: eval_degree.inner_box_columns(),
            curr_cols: 0,
        }
    }

    /// A row has columns equivalent to iterpolated polynomial degree
    fn max_cols(&self) -> u32 {
        self.max_cols
    }

    /// Returns boolean indicating whether you can insert data into the row.
    /// A row is considered fully occupied when all its columns are filled.
    fn is_free(&self) -> bool {
        self.curr_cols < self.max_cols
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
    item_data_hash_set: HashSet<(usize, u32)>,
    psi_params: PsiParams,
}

impl InnerBox {
    /// Since a single item spans across `lane_span`. InnerBox
    /// has bfv_degree / lane_span hash table rows. Remember that each `HashTableRow`
    /// has `lane_span`rows.
    fn new(psi_params: &PsiParams) -> InnerBox {
        // A single entry spans across multiple slots
        let slots_per_entry = psi_params.psi_pt.slots_required();
        let row_count = psi_params.ct_slots.0 / slots_per_entry;
        let ht_rows = (0..row_count)
            .into_iter()
            .map(|_| InnerBoxRow::new(slots_per_entry, &psi_params.eval_degree))
            .collect_vec();

        // initialise containers for data
        let label_data = Array2::<u32>::zeros((
            psi_params.ct_slots.0 as usize,
            psi_params.eval_degree.inner_box_columns() as usize,
        ));
        let item_data = Array2::<u32>::zeros((
            psi_params.ct_slots.0 as usize,
            psi_params.eval_degree.inner_box_columns() as usize,
        ));

        // println!(
        //     "Created InnerBox with {row_count} rows and {} cols",
        //     psi_params.eval_degree.inner_box_columns()
        // );

        InnerBox {
            coefficients_data: Array2::zeros((0, 0)),
            item_data,
            label_data,
            ht_rows,
            initialised: false,
            item_data_hash_set: HashSet::new(),
            psi_params: psi_params.clone(),
        }
    }

    /// Checks whether ItemLabel can be inserted in row at `index`.
    ///
    /// To insert, two conditions must be met
    /// (1) InnerBoxRow as index `row` must have an empty column.
    /// (2) Chunks of `item` in `ItemLabel` must not collide with existing entries in their respective real rows.
    fn can_insert(&self, item_label: &ItemLabel, row: usize) -> bool {
        if !self.ht_rows[row].is_free() {
            return false;
        }

        // check that none of the chunks of ItemLabel's `item` collide with existing chunks in respective real rows.
        let real_row = row * self.psi_params.psi_pt.slots_required() as usize;
        let mut can_insert = true;
        for i in real_row..real_row + self.psi_params.psi_pt.slots_required() as usize {
            let (item_chunk, _) =
                item_label.get_chunk_at_index((i - real_row) as u32, &self.psi_params.psi_pt);

            if self.item_data_hash_set.contains(&(i, item_chunk)) {
                // println!("[IB] Found chunk collision for ItemLabel. item: {}, chunk: {}, ib_row: {row}, real_row:{i}", item_label.item(), item_chunk);
                can_insert = false;
                break;
            }
        }
        can_insert
    }

    /// Insert item label at row
    fn insert_item_label(&mut self, row: usize, item_label: &ItemLabel, psi_pt: &PsiPlaintext) {
        // get next free column at InnerRow
        let col = self.ht_rows[row].next_free_col_index();
        // map InnerRow to row in container row
        let real_row = row * self.psi_params.psi_pt.slots_required() as usize;
        for i in real_row..(real_row + self.psi_params.psi_pt.slots_required() as usize) {
            // get data chunk
            let chunk_index = (i - real_row) as u32;
            let (item_chunk, label_chunk) = item_label.get_chunk_at_index(chunk_index, psi_pt);

            // println!(
            //     "[IB] Inserting ItemLabel - item:{}, chunk_index:{chunk_index}, chunk:{item_chunk}, label:{label_chunk}, InnerBox Row:{row}, Real Row:{i}",
            //     item_label.item(),
            // );

            // add the item and label chunk
            let entry = self.item_data.get_mut((i, col)).unwrap();
            *entry = item_chunk;
            let entry = self.label_data.get_mut((i, col)).unwrap();
            *entry = label_chunk;

            // add `item_chunk` as entry to item_data_hash_set for corresponding real row.
            // This is to check for collisions later.
            self.item_data_hash_set.insert((i, item_chunk));
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
            --------------------------------------
            [IB] Generating Coefficients for IB with InnerBoxRows: {},
            No. of polynomials with degree {} interpolate: {}

            ",
            self.ht_rows.len(),
            self.item_data.shape()[1],
            self.item_data.shape()[0]
        );
        let shape = self.item_data.shape();
        self.coefficients_data = Array2::<u32>::zeros((shape[0], shape[1]));
        // TODO: can we parallelise across each row as well?

        izip!(
            self.coefficients_data.outer_iter_mut(),
            self.item_data.outer_iter(),
            self.label_data.outer_iter()
        )
        .enumerate()
        .par_bridge()
        .for_each(|(index, (mut coeffs, item, label))| {
            // map real row to InnerBoxRow index
            let ibr_index = index / self.psi_params.psi_pt.slots_required() as usize;

            // limit polynomial interpolation to maximum columns occupied
            let cols_occupied = self.ht_rows[ibr_index].curr_cols as usize;

            // TODO: uncomment
            // println!("[IB] Interpolating polynomial of degree {cols_occupied}");

            let c = newton_interpolate(
                &item.as_slice().unwrap()[..cols_occupied],
                &label.as_slice().unwrap()[..cols_occupied],
                self.psi_params.psi_pt.bfv_pt as u32,
            );
            coeffs.as_slice_mut().unwrap()[..cols_occupied].copy_from_slice(&c);
        });

        // println!(
        //     "
        //     End generating coefficients
        //     ########
        //     ",
        // )
    }

    fn evaluate_ps_on_query_ct(
        &self,
        ps_powers: &HashMap<usize, Ciphertext>,
        evalutor: &Evaluator,
        ek: &EvaluationKey,
        level: usize,
    ) -> Ciphertext {
        let mut res_ct = ps_evaluate_poly(
            evalutor,
            ek,
            &ps_powers,
            &self.psi_params.ps_params,
            &self.coefficients_data,
            level,
        );

        //TODO: evalutor.mod_down_level(&mut res_ct, 0);
        // mod down to last level
        evalutor.mod_down_level(&mut res_ct, self.psi_params.bfv_moduli.len() - 1);
        res_ct
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
    psi_params: PsiParams,
    inner_box_rows: u32,
    id: usize,
}

impl BigBox {
    pub fn new(psi_params: &PsiParams, id: usize) -> BigBox {
        // rows in single inner box

        let inner_box_rows = InnerBox::max_rows(&psi_params.psi_pt, &psi_params.ct_slots);

        let segments = (psi_params.ht_size.0 + (inner_box_rows >> 1)) / inner_box_rows;
        let mut inner_boxes = vec![];
        // setup inner boxes for stack rows
        (0..segments)
            .into_iter()
            .for_each(|_| inner_boxes.push(vec![InnerBox::new(psi_params)]));

        BigBox {
            inner_boxes,
            psi_params: psi_params.clone(),
            inner_box_rows,
            id,
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

    pub fn insert_many(
        &mut self,
        item_labels: &[ItemLabel],
        item_labels_table_indices: &[Vec<u32>],
    ) {
        izip!(item_labels.iter(), item_labels_table_indices.iter()).for_each(|(il, tb_indices)| {
            self.insert(il, tb_indices[self.id] as usize);
        });
    }

    pub fn insert(&mut self, item_label: &ItemLabel, ht_index: usize) {
        let segment_index = self.ht_index_to_segment_index(ht_index);
        let inner_box_row = self.ht_index_to_inner_box_row(ht_index);

        // println!(
        //     "[BB {}] Inserting item: {} at ht_index: {}; segment_index: {}, ib_row: {}",
        //     self.id,
        //     item_label.label(),
        //     ht_index,
        //     segment_index,
        //     inner_box_row
        // );

        // Find the first InnerBox in segment that has free space at row
        let mut inner_box_index = None;
        for i in 0..self.inner_boxes[segment_index].len() {
            if self.inner_boxes[segment_index][i].can_insert(item_label, inner_box_row) {
                inner_box_index = Some(i);
                break;
            }
        }
        if inner_box_index.is_none() {
            // println!(
            //     "[BB {}] All InnerBoxes at segment {segment_index} at row {inner_box_row} are full. Creating new IB"
            //     ,
            //     self.id
            // );
            // None of the inner boxes in segment have space available at row. Create a new one.
            self.inner_boxes[segment_index].push(InnerBox::new(&self.psi_params));
            // set the index to newly inserted InnerBox
            inner_box_index = Some(self.inner_boxes[segment_index].len() - 1);
        }
        let inner_box_index = inner_box_index.unwrap();

        // insert item label
        self.inner_boxes[segment_index][inner_box_index].insert_item_label(
            inner_box_row,
            item_label,
            &self.psi_params.psi_pt,
        );

        // println!(
        //     "[BB {}] Item {} for ht_index:{ht_index} inserted; segment {segment_index}, inner_box_index {inner_box_index}, ib_row: {inner_box_row}",
        //     self.id,
        //     item_label.item()
        // );
    }

    /// Preprocesses each InnerBox
    pub fn preprocess(&mut self) {
        self.inner_boxes
            .par_iter_mut()
            .enumerate()
            .for_each(|(s_i, segment)| {
                segment
                    .par_iter_mut()
                    .enumerate()
                    .for_each(|(ib_index, ib)| {
                        println!(
                            "[BB {}] Preprocessing IB from segment {s_i} at index {ib_index}",
                            self.id,
                        );
                        ib.generate_coefficients();
                    });
            });
    }

    /// Process hash table query cts
    pub fn process_query(
        &self,
        ht_query_cts: &HashTableQueryCts,
        evaluator: &Evaluator,
        ek: &EvaluationKey,
        powers_dag: &HashMap<usize, Node>,
    ) -> HashTableQueryResponse {
        // there must be one query ciphertext (raised to different source powers) for each segment
        assert!(
            ht_query_cts.0.len() == self.inner_boxes.len() * self.psi_params.source_powers.len()
        );

        let ht_query_cts_chunked_as_source_powers = ht_query_cts
            .0
            .par_chunks_exact(self.psi_params.source_powers.len());

        let mut ht_response = Vec::new();
        ht_query_cts_chunked_as_source_powers
            .into_par_iter()
            .zip(self.inner_boxes.par_iter())
            .map(|(query_ct_powers, segment)| {
                // calculate PS powers from source powers
                // TODO: parallelizing `calculate_ps_powers_with_dag` can give speed up since it bottlenecks further multithreading. Usually there will be far less segments to process in parallel than available threads (with default parameters segments = 8).
                let ps_target_powers = calculate_ps_powers_with_dag(
                    evaluator,
                    ek,
                    &query_ct_powers,
                    &self.psi_params.source_powers,
                    self.psi_params.ps_params.powers(),
                    powers_dag,
                    &self.psi_params.ps_params,
                );

                // NOTE: We can level down here to improve the runtime for polynomial evaluation without any loss of correctness. But there exists a trade-off since levelling down will require
                // relinerization key for level 1. So level down only when run time of polynomia l evaluation is the bottleneck.
                let mut ib_responses = Vec::new();
                segment
                    .par_iter()
                    .map(|ib| ib.evaluate_ps_on_query_ct(&ps_target_powers, evaluator, ek, 0))
                    .collect_into_vec(&mut ib_responses);

                ib_responses
            })
            .collect_into_vec(&mut ht_response);

        HashTableQueryResponse(ht_response)
    }

    pub fn print_diagnosis(&self) {
        let single_ib = &self.inner_boxes[0][0];

        println!(
            "
            ------------------------------------------
            BigBox Id : {}
            ",
            self.id
        );
        println!(
            "
            No. of Segments: {}
            ",
            self.inner_boxes.len()
        );
        println!(
            "
                No. of HashTable rows per Segment (ie InnerBox): {}
                
            ",
            single_ib.ht_rows.len(),
        );
        println!(
            "
                InnerBox
                    Max no. of columns (ie data points) per InnerBox: {},
                    No. of real rows per InnerBox: {}

            ",
            single_ib.item_data.shape()[1],
            single_ib.item_data.shape()[0],
        );
        self.inner_boxes
            .iter()
            .enumerate()
            .for_each(|(segment_index, inner_boxes)| {
                println!(
                    "
                        Segment Index {segment_index} InnerBoxes count: {}
                    ",
                    inner_boxes.len()
                );
            });
        println!(
            "
            ------------------------------------------
            "
        );
    }
}

pub struct Db {
    cuckoo: Cuckoo,
    big_boxes: Vec<BigBox>,
    item_set_cache: HashSet<U256>,
    psi_params: PsiParams,
}

impl Db {
    pub fn new(psi_params: &PsiParams) -> Db {
        let cuckoo = Cuckoo::new(psi_params.no_of_hash_tables, *psi_params.ht_size);
        let big_boxes = (0..psi_params.no_of_hash_tables)
            .into_iter()
            .map(|i| BigBox::new(&psi_params, i as usize))
            .collect_vec();

        Db {
            cuckoo,
            big_boxes,
            item_set_cache: HashSet::new(),
            psi_params: psi_params.clone(),
        }
    }

    /// Inserts many ItemLabels. Uses all the cores to reduce insert time
    pub fn insert_many(&mut self, item_labels: &[ItemLabel]) {
        // TODO: check that there are no repeated items

        // hash using all cores
        let cores = rayon::current_num_threads();
        let chunk_size = item_labels.len() / cores;
        let item_labels_table_indices: Vec<Vec<u32>> = item_labels
            .par_chunks(chunk_size)
            .flat_map(|chunk_item_labels| {
                chunk_item_labels
                    .iter()
                    .map(|il| self.cuckoo.table_indices(il.item()))
                    .collect_vec()
            })
            .collect();

        // insert ItemLabels in BigBox in parallel
        self.big_boxes.par_iter_mut().for_each(|(bb)| {
            bb.insert_many(item_labels, &item_labels_table_indices);
        });
    }

    pub fn insert(&mut self, item_label: &ItemLabel) -> bool {
        // It's Private SET intersection. You cannot insert same item twice!
        if self.item_set_cache.contains(item_label.item()) {
            return false;
        }

        // get index for item for all hash tables
        let indices = self.cuckoo.table_indices(item_label.item());

        // insert item at index corresponding to hash table
        izip!(self.big_boxes.iter_mut(), indices.iter()).for_each(|(big_box, ht_index)| {
            big_box.insert(&item_label, *ht_index as usize);
        });

        self.item_set_cache.insert(item_label.item().clone());

        true
    }

    pub fn preprocess(&mut self) {
        self.big_boxes.par_iter_mut().for_each(|bb| bb.preprocess());
    }

    pub fn db_size(&self) -> usize {
        self.item_set_cache.len()
    }

    pub fn handle_query(
        &self,
        query: &Query,
        evaluator: &Evaluator,
        ek: &EvaluationKey,
        powers_dag: &HashMap<usize, Node>,
    ) -> QueryResponse {
        assert!(query.0.len() == self.psi_params.no_of_hash_tables as usize);

        let mut ht_responses = Vec::new();
        query
            .0
            .par_iter()
            .zip(self.big_boxes.par_iter())
            .map(|(ht_query_cts, bb)| {
                let ht_response = bb.process_query(ht_query_cts, evaluator, ek, powers_dag);
                ht_response
            })
            .collect_into_vec(&mut ht_responses);

        QueryResponse(ht_responses)
    }

    pub fn print_diagnosis(&self) {
        self.big_boxes.iter().for_each(|bb| {
            bb.print_diagnosis();
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::{random_u256, time_it};

    use super::*;
    use rand::thread_rng;

    #[test]
    fn bench_parallel_inner_box_gen_ceofficients() {
        let psi_params = PsiParams::default();
        let mut inner_box = InnerBox::new(&psi_params);
        let mut rng = thread_rng();
        for i in 0..InnerBox::max_rows(&psi_params.psi_pt, &psi_params.ct_slots) {
            while inner_box.ht_rows[i as usize].is_free() {
                let item_label = {
                    let item = random_u256(&mut rng);
                    let label = random_u256(&mut rng);
                    ItemLabel { item, label }
                };
                if inner_box.can_insert(&item_label, i as usize) {
                    inner_box.insert_item_label(i as usize, &item_label, &psi_params.psi_pt);
                }
            }
        }
        time_it!("Generate coefficients", inner_box.generate_coefficients(););
    }
}
