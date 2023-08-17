use crate::{poly_interpolate::newton_interpolate, ItemLabel, PsiPlaintext};
use itertools::{izip, Itertools};
use ndarray::Array2;

/// Poly Degree
struct InnerBoxCol(usize);

/// Lane count
struct InnerBoxRow(usize);

/// No. of lanes an item occupies
struct ItemLaneSpan(usize);

/// Spans multiple rows in reality
struct HashTableRow {
    span: usize,
    // equal to degree
    max_cols: usize,
    // no. of cols occupied
    curr_cols: usize,
}
impl HashTableRow {
    fn new(span: usize, max_cols: usize) -> HashTableRow {
        HashTableRow {
            span,
            max_cols,
            curr_cols: 0,
        }
    }

    fn is_free(&self) -> bool {
        self.curr_cols < self.max_cols
    }

    /// `curr_cols` indicate no. of columns occupied. So the next free index is `curr_cols` value.
    fn next_free_col_index(&self) -> usize {
        self.curr_cols
    }
}

struct InnerBox {
    coefficients_data: Array2<u32>,
    item_data: Array2<u32>,
    label_data: Array2<u32>,
    /// Each row has `lane_space` lanes
    ht_rows: Vec<HashTableRow>,
    lane_span: usize,
    eval_degree: usize,
    /// Is set to initialised when a new item is added
    initialised: bool,
    psi_pt: PsiPlaintext,
}

impl InnerBox {
    /// Since a single item spans across `lane_span`. InnerBox
    /// has bfv_degree / lane_span hash table rows. Remember that each `HashTableRow`
    /// has `lane_span`rows.
    fn new(psi_pt: &PsiPlaintext, bfv_degree: usize, eval_degree: usize) -> InnerBox {
        let lane_span = psi_pt.lane_span();
        let hash_table_rows = (0..(bfv_degree / lane_span))
            .into_iter()
            .map(|_| HashTableRow::new(lane_span, eval_degree))
            .collect_vec();
        // initialise container for data
        let label_data = Array2::<u32>::zeros((bfv_degree, eval_degree));
        let item_data = Array2::<u32>::zeros((bfv_degree, eval_degree));

        InnerBox {
            coefficients_data: Array2::zeros((0, 0)),
            item_data,
            label_data,
            ht_rows: hash_table_rows,
            lane_span,
            initialised: false,
            eval_degree,
            psi_pt: psi_pt.clone(),
        }
    }

    /// Returns whether there's space to insert an ItemLabel
    ///
    /// Space depens on whether degree columns are occupied for given row chunk
    fn can_insert(&self, index: usize) -> bool {
        self.ht_rows[index].is_free()
    }

    /// Takes in Item label and inserts otherwise reject
    ///
    /// Hash table index is the bucket to which item must be inserted. Note that a single bucket spans
    /// multiple lanes
    fn insert_item_label(
        &mut self,
        hash_table_index: usize,
        item_label: &ItemLabel,
        psi_pt: &PsiPlaintext,
    ) {
        // map index to container index
        let col = self.ht_rows[hash_table_index].next_free_col_index();

        for i in ((hash_table_index * self.lane_span)
            ..((hash_table_index * self.lane_span) + self.lane_span))
        {
            // get data chunk
            let (item_chunk, label_chunk) = item_label.get_chunk_at_index(i, psi_pt);

            // add the item and label chunk
            let entry = self.item_data.get_mut(((i, col))).unwrap();
            *entry = item_chunk;
            let entry = self.label_data.get_mut(((i, col))).unwrap();
            *entry = label_chunk;
        }

        // increase columns occupancy by 1
        self.ht_rows[hash_table_index].curr_cols += 1;
        self.initialised = true;
    }

    fn rows(lane_span: usize, bfv_degree: usize) -> usize {
        bfv_degree / lane_span
    }

    /// Iterates through all rows and generates coefficients
    ///
    /// TODO: Avoid rows that haven't been touched
    fn generate_coefficients(&mut self) {
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

/// Contains `hash_table_size / bfv_degree` InnerBoxes stacked on top of of each other.
struct BigBox {
    /// Although inner_boxes is a 2d array of `InnerBox`, can't store it as such since length of each row is a not equal
    inner_boxes: Vec<Vec<InnerBox>>,
    hash_table_size: usize,
    bfv_degree: usize,
    eval_degree: usize,
    /// rows in single inner box
    inner_box_rows: usize,
    psi_pt: PsiPlaintext,
}

impl BigBox {
    fn new(
        hash_table_size: usize,
        bfv_degree: usize,
        eval_degree: usize,
        psi_pt: &PsiPlaintext,
    ) -> BigBox {
        // rows in single inner box
        let inner_box_rows = InnerBox::rows(psi_pt.lane_span(), bfv_degree);

        let stack_rows = hash_table_size / inner_box_rows;
        let mut inner_boxes = vec![];
        // setup inner boxes for stack rows
        (0..stack_rows)
            .into_iter()
            .for_each(|_| inner_boxes.push(vec![InnerBox::new(psi_pt, bfv_degree, eval_degree)]));

        BigBox {
            inner_boxes,
            hash_table_size,
            bfv_degree,
            eval_degree,
            inner_box_rows,
            psi_pt: psi_pt.clone(),
        }
    }

    fn map_ht_index_to_stack_row(&self, ht_index: usize) -> usize {
        ht_index / self.inner_box_rows
    }

    fn map_ht_index_to_inner_box_index(&self, ht_index: usize) -> usize {
        ht_index % self.inner_box_rows
    }

    fn insert(&mut self, item_label: &ItemLabel) {
        // TODO: hash and map
        let ht_index = 0;

        let stack_row = self.map_ht_index_to_stack_row(ht_index);
        let inner_box_index = self.map_ht_index_to_inner_box_index(ht_index);

        // check whether the last inner box stored at stack row has free space
        if self.inner_boxes[stack_row]
            .last()
            .unwrap()
            .can_insert(inner_box_index)
        {
            self.inner_boxes[stack_row]
                .last_mut()
                .unwrap()
                .insert_item_label(inner_box_index, item_label, &self.psi_pt);
        } else {
            // create new inner box at stack_row and inset
            let mut inner_box = InnerBox::new(&self.psi_pt, self.bfv_degree, self.eval_degree);

            inner_box.insert_item_label(inner_box_index, item_label, &self.psi_pt);

            self.inner_boxes[stack_row].push(inner_box);
        }
    }
}
