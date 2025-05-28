// Schema definition
N::User {
    name: String,
    age: I32,
    email: String,
    INDEX name
}

N::Post {
    title: String,
    content: String,
    created_at: Date,
    INDEX title
}

N::Comment {
    text: String,
    created_at: Date
}

E::Wrote {
    From: User,
    To: Post,
    Properties: {
        timestamp: Date
    }
}

E::CommentedOn {
    From: User,
    To: Post,
    Properties: {
        timestamp: Date
    }
}

E::Replied {
    From: Comment,
    To: Comment,
    Properties: {}
}

V::DocumentEmbedding {
    content: String,
    category: String
} 