Task of private set intersection that returns the corresponding label instead of a boolean value indicating whether corresponding item is present or not reduces down to polynomial evaluation.

For example consider the set $x= [x_0, x_1, x_2, ..., x_n]$ and corresponding labels $y = [y_0, y_1, y_2, ..., y_n]$. Now let $p(x)$ be polynomial such as
$$p(x_i) = y_i \space if \space x_i \in x, \space otherwise \space random $$

Evaluating $p(x)$ must either return correct label or some random data. The idea is to evaluate $p(x)$ homomorphically on encrypted user input $x_i$.

However, we will run into issues with $p(x)$ since it returns a random value if $x_i \nin x$. To avoid such cases we will require server to return a boolean value indicating whether requested value exists or not. Or we can assume the client has some way of detecting erroneous values and can safely ignore such results. Throughout we will assume that we are in the latter case.

Now if server's set has a million values then $p(x)$ will have degree of a million. But this is problematic since evaluating such a polynomial homomorphically will require a very deep FHE circuit and will not result in an efficient construction.

One naive way to reduce polynomial degree is to divide the set into $k$ subsets and construct polynomial for each subset. Then evaluate user's input on each polynomial and return all results to the client. This increases response size by factor of $k$. As you will learn below this helps, but we cannot rely on this alone since it can blow up the response size.

## Cuckoo Hashing

Cuckoo hashing is a technique used to reduce hash collisions for storing data within a hash table with hash as the index while ensuring faster reads. The idea is to use $n$ hash tables and assign each table a different hash function. To insert, first try inserting the data in the first table. If there's a collision, insert current value and take out the existing value. Then try inserting the existing value in the next table in sequence. If there's a collision again, then insert existing value and take whatever was at the place and try inserting the value in the next table. This process continues till all tables are exhausted. It is assured that if hash functions corresponding to each table are chosen correctly, then the probability of not being able to find a spot for a value in all tables is very less. In practice, n=3 suffices for large enough hash tables.

For example, lets say we have 3 hash tables, ht1, ht2, ht3 each of size 100. Let their corresponding hash functions be h1, h2, h3. To insert a data $d_1$, first try inserting $d$ in h1 by calculating its index $i_1$ in h1 as $i_1 = h1(d) \mod 100$. Let's assume that $d_2$ already exists at $i_i$. So we insert $d_1$ at $i_1$ and take $d_2$ out and try inserting it to next table in sequence, that is $ht_2$.

Recall that the biggest issue with $p(x)$ is that its degree is of size of the set. The reason for this is that each input can be any of the values in the set held by server. With hashing we can ask client to create a single hash table of size $n$ and insert their query values at index $hash(value) \mod n$. The server can do the same. It will divide its set into $n$ sets one for each row of hash table (ie server inserts values in the next column in case collisions). With this we have reduced the polynomial degree by factor of $n$. But there's problem with using a single hash table, collisions. To query multiple items, if the user only creates one hash table then there will be many collisions. This is where cuckoo hashing comes handy. Cuckoo hashing ensures that by using multiple hash tables with different hash functions, we can insert many items without failing to find at least one index in either of the hash tables for any item with high probability. If client uses $k$ hash tables, server now needs to map each value in its set for each of the $k$ hash functions. This is because it cannot learn in which of the hash tables a given user query item exists. 

GIVE AN EAMPLE

Server ends up with $k$ different hash tables, but calling them hash table is incorrect since server appends values upon collision. We refer to them as BigBoxes. To reduce polynomial degree further, server divides columns of each BigBox into size of appropriate polynomial degree. 

## BigBox

Let hash function corresponding to BigBox be $h1(.)$ and let size of hash table be $n$. All elements are inserted at $index = h1(data) \mod n$ and collisions are simply appended. Thus BigBox may look like:

$$
\begin{array}{ccc}
a_{0,0} & a_{0,1} & \cdots \\
a_{1,0} & a_{1,1} & \cdots \\
\vdots \\
a_{500,0} & a_{500,1} & \cdots \\
\vdots
\end{array}
$$

where $h1(.) \mod n$ for any $row_i$ are equal. 

In practice a single hash table does not fit in a single query ciphertext, thus we need to split BigBox into multiple segments. For example, if a single ciphertext can fit in only $k$ rows of hash table then we must split the BigBox into segments each with $k$ rows. In this case, BigBox may look like

$$
\begin{array}{}
a_{0,0} & a_{0,1} & \cdots \\
a_{1,0} & a_{1,1} & \cdots \\
\vdots\\
a_{k-1,0} & a_{k-1,1} & \cdots \\
- & - & - \\
a_{k,0} & a_{k,1} & \cdots \\
a_{k+1,0} & a_{k+1,1} & \cdots \\
\vdots \\
a_{2k-1,0} & a_{2k-1,1} & \cdots \\
- & - & - \\
\vdots \\
\end{array}
$$
As server's set increases, no of columns occupied at each row will increase. To avoid polynomial degree to increase, we fix the polynomial degree to a value $EvalDegree$. This means we further need to divide a segment into multiple sets of columns, each with $EvalDegree+1$ columns (polynomial of $EvalDegree$ can interpolate $EvalDegree + 1$ data points). 

Zooming into a single segment:  

$$
\begin{array}{}
a_{0,0} & a_{0,1} & \cdots & a_{0, ed} && | && a_{0,ed+1} & a_{0,ed+2} & \cdots \\
a_{1,0} & a_{1,1} & \cdots & a_{1, ed} && | && \cdots\\
\vdots & && && | && \vdots \\ 
a_{k-1,0} & a_{k-1,1} & \cdots & a_{k-1, ed} && | \\
\end{array}
$$
Notice that we divided the segment into smaller boxes each with $EvalDegree + 1$ columns. We call these smaller boxes, InnerBox. Also notice that since no. of values appended to any row of BigBox is variable, each segment may have varying no. of InnerBoxes. 

## Polynomial Interpolation

Recall that a single row of InnerBox has $EvalDegree + 1$ values and we consider each row as independent data points. We require to interpolate polynomial over data points of a single row and to do that we use Newton's polynomial interpolation method.
### Newton's interpolation method

Consider the data points $x = [x_0, x_1, x_2, ..., x_{n-1}]$ and $y = [y_0, y_1, y_2, ..., y_{n-1}]$ . Using newton's interpolation we can interpolate a polynomial that evaluates to $y_i$ as $p(x_i)$.

$$p(x) = a_0 + a_1(x-x_0) + ... + a_{n-1}(x-x_{n-2})(x-x_{n-3})...(x-x_0)$$
Notice that $a_0 = y_0$

To figure out $a_i$ we must notice the following pattern:

$$f(x_1) = a_0 + a_1(x - x_0)$$
Since $f(x_1) = y_1$
$$ a_1 = \frac{y_1 - y_0}{(x_1 - x_0)}$$
Moreover since $f(x_2) = y_2$
$$ a_2 = \frac{\frac{y_2 - y_1}{(x_2 - x_1)} - \frac{y_1 - y_0}{(x_1 - x_0)}}{x_2 - x_0}$$
Notice that $a_2$ depends on $a_1$. And the pattern continues.

Now if we set $[y_{i}, y_{i-1}] = \frac{y_{i} - y_{i-1}}{x_{i} - x_{i-1}}$, then we can rewrite a_2 as
$$ a_2 = \frac{[y_2, y_1] - [y_1, y_0]}{x_2 - x_0}$$

We call this notation _divided differences_. We can further improve upon the notation as:
$$[y_k, y_{k-1}, ..., y_0] = \frac{[y_k,...,y_1] - [y_{k-1},...,y0]}{x_k - x_0}$$
Thus we can denote $a_2$ as $[y_2, y_1, y_0]$.

#### Divided differences matrix

Notice that divided differences (ie $a_i$ values) depend on pervious values. Thus we can construct the following matrix to calculate $a_i$'s. I will illustrate the matrix using 5 data points.

$$
\begin{matrix}
a_0 & a_1 & a_2 & a_3 & a_4 \\
y_0 & [y_1, y_0] & [y_2, y_1, y_0] & [y_3, y_2, y_1, y_0] & [y_4, y_3, y_2, y_1, y_0]\\
y_1 & [y_2, y_1] & [y_3, y_2, y_1] & [y_4, y_3, y_2, y_1] & 0\\
y_2 & [y_3, y_2] & [y_4, y_3, y_2] & 0 & 0\\
y_3 & [y_4, y_3] & 0 & 0 & 0\\
y_4 & 0 & 0 & 0 & 0\\
\end{matrix}
$$

Notice that values in $col_{i+1}$ are calculated using values in $col_{i}$. For example,
$$a_4 = [y_4, y_3, y_2, y_1, y_0] = \frac{[y_4, y_3, y_2, y_1] - [y_3, y_2, y_1, y_0]}{x_4 - x_0}$$

### Horner'r rule to get coefficients

Using newton's interpolation we get p(x) of form:
$$p(x) = a_0 + a_1(x-x_0) + ... + a_{n-1}(x-x_{n-2})(x-x_{n-3})...(x-x_0)$$

But for efficient homomorphic polynomial evaluation we will require $p(x)$ in form
$$p(x) = \sum_{i=0}^{n-1} c_i x^i$$

We apply Horner's rule to get values for $c_i$'s. We start with constant polynomial with value of $a_{n-1} = [y_{n-1}, ..., y_0]$, multiply it with polynomial $(x - x_{n-1})$, and add the next value of $a_i$ in sequence, that is $a_{n-2}$. We repeat the procedure till $a_0$. For intuition we are constructing $p(x)$ as
$$p(x) = (((a_{n-1}(x-x_{n-2})) + a_{n-2})(x-x_{n-3}) + a_{n-3})...+a_{n-0}$$

```
/// p_x is viewed through its coefficients in ascending order of degree
let p_x = 0
for i in n-1..1 {
	p_x += a_{i}
	p_x *= (x - x_{i-1})
}
p_x += a_0
```

# Implementation

Let us parameterise no. of hash tables in cuckoo hashing with $h$. The server processes the entire dataset for each of $h$ hash table and produces $h$ BigBoxes. This implies each ItemLabel is present in some of row of each BigBox. 

## BigBox

BigBox is equivalent of hash table on the server. Unlike hash table it is 2 dimensional matrix with as many as rows in hash table and arbitrary no. of columns. 

Since a single ciphertext does not fit an entire hash table, we need to segment BigBox into different sets of consecutive rows. We call each set a Segment. Each Segment has $segment_{rows} = CiphertextSlots / PtSlots$ rows. 

Segment itself does not own any rows, instead it is a vector of InnerBoxes. A single InnerBox has as $EvalDegree$ no. of columns and a vector of InnerBoxes in a Segment allows Segment to have arbitrarily no. of columns. 

DIAGRAM PRESETING SEGMENTS AND INNERBOXES

To inset ItemLabel into a BigBox, first hash item value of ItemLabel using BigBox's hash function and map the result to to one the rows. Let $index = result \mod n$. To map $index$ to one of the segments, calculate segment index $segment_i = index / segment_{rows}$ and to map index to one of the rows within segment calculate $row = index \mod segment_{rows}$. Insert the ItemLabel at $row$ of segment at index $segment_i$. 

## InnerBox

Segment stores a vector of InnerBoxes. Each InnerBox has as many as $segment_{rows}$ and $EvalDegree + 1$ columns. As mentioned, storing multiple InnerBoxes allows Segment to have arbitrary no. of columns. 

To insert ItemLabel at $row$ of segment, insert it in the $row$ of one of the InnerBoxes. To find the InnerBox to insert ItemLabel into, iterate through all InnerBoxes in sequence and find the first InnerBox that satisfies the following conditions:
1. InnerBox has free column at $row$.
2. Upon chunking item value of ItemLabel into chunks, the chunk value should not already exist in its respective $realRow$. This is to avoid having duplicate $x$ entries in data points that map to different $y$ points during polynomial interpolation. The reason for this is obvious, you cannot have a polynomial output two different $y$ values on same $x$ value.
If a InnerBox is found, then insert the ItemLabel, otherwise create a new InnerBox and insert the ItemLabel.

Since a single ItemLabel cannot fit in plaintext space of ciphertext, a single $row$  must span across multiple $realRows$ (as many as $PtSlots$ rows). Thus InnerBox has $CtSlots$ $realRows$, which is equivalent to $SegmentRows * PtSlots$. Value of ItemLabel is chunked into as many $PtSlots$ chunks and the chunks are inserted across $PtSlots$ $realRows$. 

The rationale for having InnerBox is that it eases polynomial interpolation at runtime. Since InnerBox has $EvalDegree + 1$ columns, polynomial of degree $EvalDegree$ can be evaluated/interpolated on a InnerBox independently. Moreover, at runtime each column (ie the coefficients of polynomial) can be converted to BFV plaintext easily. 




