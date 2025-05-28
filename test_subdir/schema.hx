// Different schema for subdirectory testing
N::Product {
    name: String,
    price: F64,
    category: String,
    INDEX name
}

N::Customer {
    name: String,
    email: String,
    phone: String,
    INDEX email
}

E::Purchased {
    From: Customer,
    To: Product,
    Properties: {
        quantity: I32,
        purchase_date: Date
    }
} 