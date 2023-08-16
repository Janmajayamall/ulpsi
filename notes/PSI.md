----
Cuckoo Hashing

Given 3 hash functions h1, h2, h3 we hash each item with each of the hash functions and create 3 different databases. This increases the storage on server by a factor of 3, but reduces request computation and communication cost eneromously. 

The reason for the reduction is that instead of having to match every single item in client's set with every single item in server's set, we reduce it to a lot less by mapping each item to determinic indices within the hash table. However this increases the chances of collision and this is exactly where cuckoo hashing comes handy. If there is collision then kick the current value out, insert the new and hash and insert the old value using the next hash function in sequence. Since server hashes all values by all 3 hash functions and insert in all 3 dbs the value is guaranteed to exist at the expected index. 

----------
Polynomial interpolation

Think of DB with rows equal to hash table size. Server adds a new item to mapped rows. If there exists an item at the row and server adds to the next column. 

For polynomial interpolation the columns for each row are divided in sets of max degree and interpolated separately. 

