Task of private set intersection that returns the corresponding label instead of a boolean value indicating whether corresponding item is present or not reduces down to polynomial evaluation.

For example consider the set $x= [x_0, x_1, x_2, ..., x_n]$ and corresponding labels $y = [y_0, y_1, y_2, ..., y_n]$. Now let p(x) be polynomial such as
$$p(x_i) = y_i \space if \space x_i \in x, \space otherwise \space random $$
Now evaluating p(x) must either return correct label or some random data. The idea is to evaluate p(x) homomorphically with encrypted user input x.

However, we will run into issues with p(x) since it returns a random value if $x_i \nin x$. To avoid such cases we will require server to return a boolean value indicating whether requested value exists or not. Or we can assume the client has some way of detecting erroneous values and can safely ignore such results. Throughout we will assume that we are in the latter case.

Now if server set has a million values then p(x) will have degree of a million. But this is problematic since evaluating such a polynomial homomorphically will require a very deep FHE circuit and will not result in an efficient construction.

One naive way to reduce polynomial degree is to divide the set into $k$ subsets and construct polynomial for each subset. Then evaluate user's input on each polynomial and return all results to the client. This increases response size by factor of $k$. As you will learn below this helps, but we cannot rely on this alone since it can blow up the response size.

## Cuckoo Hashing

Cuckoo hashing is a technique used to reduce hash collisions for storing data with in a hash table with hash as the index while ensuring faster reads. The idea is to use $n$ hash tables and assign each table a different hash function. To insert, first try inserting the data in the first table. If there's a collision, then kick the existing value out and try inserting the existing value in the next table in sequence. If there's a collision again, then insert existing value and take whatever was at the place and try inserting the value in the next table. This process continues till all tables are exhausted. It is assured that if hash functions corresponding to each table are chosen correctly, then the probability of not being able to find a spot for a value in all tables is very less. In practice, n=3 suffices.

For example, lets say we have 3 hash tables, ht1, ht2, ht3 each of size 100. Let their corresponding hash functions be h1, h2, h3. To insert a data $d_1$, you will first try inserting $d$ in h1 by calculating its index $i_1$ in h1 as $i_1 = h1(d) \mod 100$. Let's assume that d_2 already exists at $i_i$. So we insert d_1 at i_1 and take d_2 out and try inserting it to next table in sequence, that is ht_2.

Recall that the biggest issue with p(x) is that its degree is of size of the set. The reason for this is that each input can be any of the values in the set held by server. With hashing we can ask client to create a single hash table of size $n$ and insert their query values at index $hash(value) \mod n$. The server can do the same. It will divide its set into $n$ sets one for each row of hash table (ie server inserts values in the next column in case collisions). With this we have reduced the polynomial degree by factor of $n$. But there's problem with using a single hash table, collisions. To query multiple items, if the user only creates one hash table then there will be many collisions. This is where cuckoo hashing comes handy. Cuckoo hashing ensures that by using multiple hash tables with different hash functions, we can insert many items without failing to find at least one index in either of the hash tables for any item with high probability. If client uses $k$ hash tables, server now needs to map each value in its set for each of the $k$ hash functions. This is because it cannot learn in which of the hash tables the user query exists. 

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

In practice a single hash table does not fit in a single ciphertext, thus we need to split BigBox into multiple segments. For example, if a single ciphertext can fit in only $k$ rows of hash table then we must split the BigBox into segments each with $k$ rows. In this case, BigBox may look like

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
As server's set increases, no of columns occupied at each row will increase. To avoid polynomial degree to increase, we fix the polynomial degree to a value $EvalDegree$. This means we further need to divide a segment into multiple sets of columns, each with $EvalDegree+1$ columns (polynomial of $EvalDegree$ can interpolate $EvalDegree + 1$ data points). Zooming into a single segment:  

$$
\begin{array}{}
a_{0,0} & a_{0,1} & \cdots & a_{0, ed} && | && a_{0,ed+1} & a_{0,ed+2} & \cdots \\
a_{1,0} & a_{1,1} & \cdots & a_{1, ed} && | && \cdots\\
\vdots & && && | && \vdots \\ 
a_{k-1,0} & a_{k-1,1} & \cdots & a_{k-1, ed} && | \\
\end{array}
$$
Notice that we divided the segment into smaller boxes each with $EvalDegree + 1$ columns. We call these smaller boxes, InnerBox. Also notice that since no. of values appended to any row of BigBox is variable, each segment may have varying no. of InnerBoxes. 


## Newton's interpolation method

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

### Divided differences matrix

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

## Horner'r rule to get coefficients

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

The entire dataset is processed using three hash functions to create 3 BigBoxes. Each ItemLabel is present in all three boxes at a given index of the hash table. 

## BigBox

BigBox is the hash table that stores data. It has as many rows as hash table size. Each new item label that lands on the same row is appended after that last item inserted (ie in next free column). 

To ease runtime processing, BigBox is further divided into 2D InnerBoxes. A row in BigBox corresponds to a InnerBoxRow of InnerBox. This implies that viewing BigBox as 2d array of InnerBoxes, there are as many as `Hash table size / No. of InnerBoxRows in InnerBox` rows of InnerBoxes. Let's call each row of InnerBox a segment. Each segment can contain many InnerBoxes and this cannot be determined at start, since arbitrarily no. of values may be inserted in each row. 

To insert an ItemLabel at a row, first map it to a segment, that is an InnerBoxRow. Then find the first InnerBox in the segment with free space at the mapped InnerBoxRow and proceed to insert the ItemLabel at InnerBoxRow in the found InnerBox. If none of the InnerBoxes in the segment has free columns, the create a new one, append it to the segment and insert the ItemLabel. 

Note that the insertion procedure implies that each segment might have different no. of InnerBoxes. 

## InnerBox

InnerBox has CtSlots/PsiPtSlots InnerBoxRows. A single InnerBoxRows spans across multiple real rows since a single ItemLabel value is stored across multiple rows (PsiPtSlots to be exact). More concretely, InnerBox contains 3 (one for item, one for label, and one for coefficients) u32 2D array of of dimension `CtSlots x EvalDegree`. 

The rationale for structuring inner box as such is that it eases polynomial interpolation and convert each column into BFV Plaintexts. At runtime, for homomorphic polynomial interpolation on encrypted query, each column in InnerBox can be easily converted to BfvPlaintexts for homomorphic polynomial interpolation.

There's a subtle issue with the way we handle inserts above. Since each real row is used for polynomial interpolation, what if there exists two same values in different columns in item's 2d array. This implies we expect the polynomial to ouput two different values on the same input. This isn't possible. To avoid this issue, instead of just finding the first InnerBox that has space for ItemLabel we should also assure that none of the chunks of item collide with existing chunks in their respective real rows. 


