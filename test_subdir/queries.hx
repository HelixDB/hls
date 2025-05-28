// Queries for the subdirectory schema
QUERY getProductsByCategory(category: String) =>
    products <- N<Product>({category: category})
    RETURN products::{name, price}

QUERY getCustomerPurchases(email: String) =>
    customer <- N<Customer>({email: email})
    purchases <- customer::Out<Purchased>
    RETURN purchases::{name, price, quantity}

// This should cause an error - User doesn't exist in this schema
QUERY badQuery() =>
    user <- N<User>
    RETURN user

// This should work - Product exists in this schema
QUERY goodQuery() =>
    product <- N<Product>
    RETURN product::{name, price} 