----
Cuckoo Hashing

Given 3 hash functions h1, h2, h3 we hash each item with each of the hash functions and create 3 different databases. This increases the storage on server by a factor of 3, but reduces request computation and communication cost eneromously. 

The reason for the reduction is that instead of having to match every single item in client's set with every single item in server's set, we reduce it to a lot less by mapping each item to determinic indices within the hash table. However this increases the chances of collision and this is exactly where cuckoo hashing comes handy. If there is collision then kick the current value out, insert the new and hash and insert the old value using the next hash function in sequence. Since server hashes all values by all 3 hash functions and insert in all 3 dbs the value is guaranteed to exist at the expected index. 

----------
Polynomial interpolation

Think of DB with rows equal to hash table size. Server adds a new item to mapped rows. If there exists an item at the row and server adds to the next column. 

For polynomial interpolation the columns for each row are divided in sets of max degree and interpolated separately. 

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

We call this notation *divided differences*. We can further improve upon the notation as: 
$$[y_k, y_{k-1}, ..., y_0] = \frac{[y_k,...,y_1] - [y_{k-1},...,y0]}{x_k - x_0}$$
Thus we can denote $a_2$ as $[y_2, y_1, y_0]$.

### Divided differences matrix

Notice that divided differences (ie $a_i$ values) depend on pervious values. Thus we can construct the following matrix to calculate $a_i$'s. I will illustrate the matrix using 5 data points. 
$$\begin{matrix}  
a_0 & a_1 & a_2 & a_3 & a_4 \\
y_0 & [y_1, y_0] & [y_2, y_1, y_0] & [y_3, y_2, y_1, y_0] & [y_4, y_3, y_2, y_1, y_0]\\  
y_1 & [y_2, y_1] & [y_3, y_2, y_1] & [y_4, y_3, y_2, y_1] & 0\\  
y_2 & [y_3, y_2] & [y_4, y_3, y_2] & 0 & 0\\  
y_3 & [y_4, y_3] & 0 & 0 & 0\\  
y_4 & 0 & 0 & 0 & 0\\  
\end{matrix}$$
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


